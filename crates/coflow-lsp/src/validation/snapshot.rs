use super::{is_cfd_path, OpenDocument};
use crate::definition::CfdDefinitionIndex;
use crate::diagnostics::{
    label_uri, lsp_diagnostic, lsp_error_diagnostic, lsp_label_location, preferred_diagnostic_uri,
};
use crate::state::LspBuild;
use crate::uri::path_to_file_uri;
use coflow_api::{DiagnosticSet, SourceLocationSpec};
use coflow_cfd::parse_cfd;
use coflow_project::{discover_directory_files, normalize_path, Project};
use coflow_runtime::{compile_schema_project_with_overrides, SchemaSourceOverride};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ValidationRevision(u64);

impl ValidationRevision {
    pub(super) const INITIAL: Self = Self(0);

    pub(super) fn next(self) -> Result<Self, String> {
        self.0
            .checked_add(1)
            .map(Self)
            .ok_or_else(|| "LSP validation revision overflow".to_string())
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ValidationInput {
    pub(super) revision: ValidationRevision,
    project: Project,
    project_diagnostics: Option<DiagnosticSet>,
    open_documents: BTreeMap<PathBuf, OpenDocument>,
}

impl ValidationInput {
    pub(super) fn new(
        revision: ValidationRevision,
        project: &Project,
        project_diagnostics: Option<&DiagnosticSet>,
        open_documents: &BTreeMap<PathBuf, OpenDocument>,
    ) -> Self {
        Self {
            revision,
            project: project.clone(),
            project_diagnostics: project_diagnostics.cloned(),
            open_documents: open_documents.clone(),
        }
    }

    pub(crate) const fn revision(&self) -> ValidationRevision {
        self.revision
    }
}

pub(crate) struct ValidationSnapshot {
    pub(super) revision: ValidationRevision,
    pub(super) build: Option<LspBuild>,
    pub(super) diagnostics: BTreeMap<String, Vec<Value>>,
    pub(super) active_uris: BTreeSet<String>,
    pub(super) document_versions: BTreeMap<String, i64>,
    pub(super) cfd_documents: BTreeMap<PathBuf, CfdDocumentSnapshot>,
}

pub(super) struct CfdDocumentSnapshot {
    pub(super) source: String,
    pub(super) ast: coflow_cfd::CfdAst,
}

impl ValidationSnapshot {
    pub(crate) const fn empty(revision: ValidationRevision) -> Self {
        Self {
            revision,
            build: None,
            diagnostics: BTreeMap::new(),
            active_uris: BTreeSet::new(),
            document_versions: BTreeMap::new(),
            cfd_documents: BTreeMap::new(),
        }
    }
}

pub(crate) fn build_snapshot(input: &ValidationInput) -> ValidationSnapshot {
    let mut snapshot = ValidationSnapshot::empty(input.revision);
    add_open_documents(&mut snapshot, &input.open_documents);

    if let Some(diagnostics) = &input.project_diagnostics {
        add_diagnostic_set(
            &mut snapshot,
            diagnostics,
            &BTreeMap::new(),
            &input.project.config_path,
        );
        return snapshot;
    }

    let schema_files = match input.project.schema_files() {
        Ok(files) => files,
        Err(diagnostics) => {
            add_diagnostic_set(
                &mut snapshot,
                &diagnostics,
                &BTreeMap::new(),
                &input.project.config_path,
            );
            return snapshot;
        }
    };
    let schema_by_path = schema_files
        .iter()
        .map(|file| {
            (
                normalize_path(&file.canonical_path),
                (file.module_id.clone(), file.canonical_path.clone()),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let preferred_uris = preferred_diagnostic_uris(&input.open_documents, &schema_by_path);
    for (_, path) in schema_by_path.values() {
        snapshot
            .active_uris
            .insert(preferred_diagnostic_uri(&preferred_uris, path));
    }

    let mut overrides = Vec::new();
    for (normalized_path, document) in &input.open_documents {
        if let Some((module_id, _)) = schema_by_path.get(normalized_path) {
            overrides.push(SchemaSourceOverride {
                requested_module: Some(module_id.clone()),
                normalized_path: normalized_path.clone(),
                source: document.text.clone(),
            });
        } else if !is_cfd_path(normalized_path) {
            snapshot.diagnostics.insert(
                document.uri.clone(),
                vec![lsp_error_diagnostic(
                    "CFT-LSP",
                    "file is not part of the configured CFT schema",
                )],
            );
        }
    }

    let (cfd_sources, cfd_failures) = add_cfd_documents(&mut snapshot, input);

    let raw_build = match compile_schema_project_with_overrides(&input.project, &overrides) {
        Ok(build) => build,
        Err(diagnostics) => {
            add_diagnostic_set(
                &mut snapshot,
                &diagnostics,
                &preferred_uris,
                &input.project.config_path,
            );
            return snapshot;
        }
    };
    add_diagnostic_set(
        &mut snapshot,
        &raw_build.diagnostics,
        &preferred_uris,
        &input.project.config_path,
    );

    for (_, module) in raw_build.modules.modules() {
        snapshot
            .active_uris
            .insert(preferred_diagnostic_uri(&preferred_uris, module.path()));
    }
    if cfd_failures.is_empty() {
        let definitions =
            CfdDefinitionIndex::from_documents(cfd_sources.iter().filter_map(|source| {
                snapshot
                    .cfd_documents
                    .get(&source.path)
                    .map(|document| (source.uri.as_str(), document.source.as_str(), &document.ast))
            }));
        snapshot.build = Some(LspBuild::new(raw_build).with_cfd_definitions(definitions));
    }
    snapshot
}

fn add_cfd_documents(
    snapshot: &mut ValidationSnapshot,
    input: &ValidationInput,
) -> (Vec<CfdProjectSource>, Vec<CfdSourceFailure>) {
    let (sources, failures) = collect_cfd_sources(&input.project, &input.open_documents);
    for source in &sources {
        let (ast, errors) = parse_cfd(&source.text);
        if let Some(document) = input.open_documents.get(&source.path) {
            snapshot.diagnostics.insert(
                document.uri.clone(),
                crate::cfd::syntax_diagnostics(&source.text, &errors),
            );
        }
        snapshot.cfd_documents.insert(
            source.path.clone(),
            CfdDocumentSnapshot {
                source: source.text.clone(),
                ast,
            },
        );
    }
    for failure in &failures {
        snapshot.active_uris.insert(failure.uri.clone());
        snapshot
            .diagnostics
            .entry(failure.uri.clone())
            .or_default()
            .push(lsp_error_diagnostic("CFD-LSP", &failure.message));
    }
    (sources, failures)
}

fn add_open_documents(
    snapshot: &mut ValidationSnapshot,
    open_documents: &BTreeMap<PathBuf, OpenDocument>,
) {
    for document in open_documents.values() {
        snapshot.active_uris.insert(document.uri.clone());
        if let Some(version) = document.version {
            snapshot
                .document_versions
                .insert(document.uri.clone(), version);
        }
    }
}

fn preferred_diagnostic_uris(
    open_documents: &BTreeMap<PathBuf, OpenDocument>,
    schema_by_path: &BTreeMap<PathBuf, (String, PathBuf)>,
) -> BTreeMap<PathBuf, String> {
    open_documents
        .iter()
        .filter(|(path, _)| schema_by_path.contains_key(*path))
        .map(|(path, document)| (path.clone(), document.uri.clone()))
        .collect()
}

fn add_diagnostic_set(
    snapshot: &mut ValidationSnapshot,
    diagnostics: &DiagnosticSet,
    preferred_uris: &BTreeMap<PathBuf, String>,
    fallback_path: &Path,
) {
    for diagnostic in diagnostics {
        let uri = diagnostic
            .primary
            .as_ref()
            .map(|label| lsp_label_location(&label.location))
            .map_or_else(
                || preferred_diagnostic_uri(preferred_uris, fallback_path),
                |location| label_uri(&location, preferred_uris),
            );
        snapshot.active_uris.insert(uri.clone());
        snapshot
            .diagnostics
            .entry(uri)
            .or_default()
            .push(lsp_diagnostic(diagnostic));
    }
}

struct CfdProjectSource {
    path: PathBuf,
    uri: String,
    text: String,
}

struct CfdSourceFailure {
    uri: String,
    message: String,
}

fn collect_cfd_sources(
    project: &Project,
    open_documents: &BTreeMap<PathBuf, OpenDocument>,
) -> (Vec<CfdProjectSource>, Vec<CfdSourceFailure>) {
    let mut sources = Vec::new();
    let mut failures = Vec::new();
    for source in &project.config.sources {
        let SourceLocationSpec::Path(path) = source.location() else {
            continue;
        };
        let resolved = project.resolve_path(path);
        if resolved.is_dir() {
            match discover_directory_files(&resolved) {
                Ok(paths) => {
                    for path in paths {
                        if is_cfd_path(&path) {
                            collect_cfd_source(&path, open_documents, &mut sources, &mut failures);
                        }
                    }
                }
                Err(err) => failures.push(CfdSourceFailure {
                    uri: path_to_file_uri(err.path()),
                    message: err.to_string(),
                }),
            }
        } else if is_cfd_path(&resolved) {
            collect_cfd_source(&resolved, open_documents, &mut sources, &mut failures);
        }
    }

    let mut indexed_paths = sources
        .iter()
        .map(|source| source.path.clone())
        .collect::<BTreeSet<_>>();
    for (path, document) in open_documents {
        if is_cfd_path(path) && indexed_paths.insert(path.clone()) {
            sources.push(CfdProjectSource {
                path: path.clone(),
                uri: document.uri.clone(),
                text: document.text.clone(),
            });
        }
    }
    sources.sort_by(|left, right| left.path.cmp(&right.path));
    (sources, failures)
}

fn collect_cfd_source(
    path: &Path,
    open_documents: &BTreeMap<PathBuf, OpenDocument>,
    sources: &mut Vec<CfdProjectSource>,
    failures: &mut Vec<CfdSourceFailure>,
) {
    let normalized = normalize_path(path);
    if let Some(document) = open_documents.get(&normalized) {
        sources.push(CfdProjectSource {
            path: normalized,
            uri: document.uri.clone(),
            text: document.text.clone(),
        });
        return;
    }
    match std::fs::read_to_string(path) {
        Ok(text) => sources.push(CfdProjectSource {
            uri: path_to_file_uri(&normalized),
            path: normalized,
            text,
        }),
        Err(err) => failures.push(CfdSourceFailure {
            uri: path_to_file_uri(&normalized),
            message: format!("failed to read CFD source `{}`: {err}", path.display()),
        }),
    }
}
