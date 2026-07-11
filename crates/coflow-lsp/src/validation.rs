use coflow_api::{DiagnosticSet, SourceLocationSpec};
use coflow_cfd::{parse_cfd, CfdAst};
use coflow_cft::CftContainer;
use coflow_project::{
    dedupe_cft_diagnostics, diagnostic_set_from_cft, normalize_path, Project,
};
use coflow_runtime::{compile_schema_project_with_overrides, SchemaSourceOverride};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::diagnostics::{
    label_uri, lsp_diagnostic, lsp_error_diagnostic, lsp_label_location, preferred_diagnostic_uri,
};
use crate::state::{LspBuild, LspDocument};
use crate::uri::path_to_file_uri;
use crate::{cfd, path_from_file_uri};

pub(crate) struct LspValidationCore {
    project: Project,
    open_documents: BTreeMap<PathBuf, OpenDocument>,
    published_uris: BTreeSet<String>,
    build: Option<LspBuild>,
}

#[derive(Debug)]
pub(crate) struct OpenDocument {
    pub(crate) uri: String,
    pub(crate) text: String,
}

pub(crate) struct DiagnosticPublication {
    pub(crate) uri: String,
    pub(crate) diagnostics: Vec<Value>,
}

pub(crate) enum LspRequestDocument<'a> {
    Cfd(CfdRequestDocument<'a>),
    Cft {
        build: &'a LspBuild,
        document: &'a LspDocument,
    },
    Missing,
}

pub(crate) struct CfdRequestDocument<'a> {
    pub(crate) source: String,
    pub(crate) ast: CfdAst,
    pub(crate) schema: Option<&'a CftContainer>,
    pub(crate) build: Option<&'a LspBuild>,
}

pub(crate) struct CfdProjectSource {
    path: PathBuf,
    pub(crate) uri: String,
    pub(crate) text: String,
}

impl LspValidationCore {
    pub(crate) fn new(project: Project) -> Self {
        Self {
            project,
            open_documents: BTreeMap::new(),
            published_uris: BTreeSet::new(),
            build: None,
        }
    }

    #[cfg(test)]
    pub(crate) const fn open_documents(&self) -> &BTreeMap<PathBuf, OpenDocument> {
        &self.open_documents
    }

    pub(crate) const fn build(&self) -> Option<&LspBuild> {
        self.build.as_ref()
    }

    pub(crate) fn schema(&self) -> Option<&CftContainer> {
        self.build
            .as_ref()
            .and_then(|build| build.schema.container.as_ref())
    }

    pub(crate) fn open_document(
        &mut self,
        uri: String,
        text: String,
    ) -> Result<Vec<DiagnosticPublication>, String> {
        let Some(path) = path_from_file_uri(&uri) else {
            return Ok(Vec::new());
        };
        self.open_documents
            .insert(normalize_path(&path), OpenDocument { uri, text });
        self.validate_project()
    }

    pub(crate) fn change_document(
        &mut self,
        uri: String,
        text: String,
    ) -> Result<Vec<DiagnosticPublication>, String> {
        let Some(path) = path_from_file_uri(&uri) else {
            return Ok(Vec::new());
        };
        let normalized = normalize_path(&path);
        self.open_documents
            .entry(normalized)
            .and_modify(|document| document.text.clone_from(&text))
            .or_insert(OpenDocument { uri, text });
        self.validate_project()
    }

    pub(crate) fn close_document(
        &mut self,
        uri: &str,
    ) -> Result<Vec<DiagnosticPublication>, String> {
        let Some(path) = path_from_file_uri(uri) else {
            return Ok(Vec::new());
        };
        self.open_documents.remove(&normalize_path(&path));
        let mut publications = vec![self.record_publication(uri.to_string(), Vec::new())];
        publications.extend(self.validate_project()?);
        Ok(publications)
    }

    pub(crate) fn validate_project(&mut self) -> Result<Vec<DiagnosticPublication>, String> {
        let schema_files = match self.project.schema_files() {
            Ok(files) => files,
            Err(diagnostics) => {
                let fallback_path = self.project.config_path.clone();
                return Ok(self.diagnostic_set_publications(
                    &diagnostics,
                    &BTreeMap::new(),
                    &fallback_path,
                ));
            }
        };
        let mut schema_by_path = BTreeMap::new();

        for file in &schema_files {
            schema_by_path.insert(
                normalize_path(&file.canonical_path),
                (file.module_id.clone(), file.canonical_path.clone()),
            );
        }

        let mut overrides = Vec::new();
        let mut non_schema_diagnostics = Vec::new();

        for (normalized_path, document) in &self.open_documents {
            if let Some((module_id, _)) = schema_by_path.get(normalized_path) {
                overrides.push(SchemaSourceOverride {
                    requested_module: Some(module_id.clone()),
                    normalized_path: normalized_path.clone(),
                    source: document.text.clone(),
                });
            } else if is_cfd_path(normalized_path) {
                let (_, errors) = parse_cfd(&document.text);
                let diagnostics = cfd::syntax_diagnostics(&document.text, &errors);
                non_schema_diagnostics.push((document.uri.clone(), diagnostics));
            } else {
                non_schema_diagnostics.push((
                    document.uri.clone(),
                    vec![lsp_error_diagnostic(
                        "CFT-LSP",
                        "file is not part of the configured CFT schema",
                    )],
                ));
            }
        }

        let preferred_uris = self.preferred_diagnostic_uris(&schema_by_path);
        let raw_build = match compile_schema_project_with_overrides(&self.project, &overrides) {
            Ok(build) => build,
            Err(diagnostics) => {
                let fallback_path = self.project.config_path.clone();
                return Ok(self.diagnostic_set_publications(
                    &diagnostics,
                    &preferred_uris,
                    &fallback_path,
                ));
            }
        };
        let build = LspBuild::new(raw_build);
        let diagnostics = dedupe_cft_diagnostics(build.schema.diagnostics.clone());
        let mut by_uri: BTreeMap<String, Vec<Value>> = BTreeMap::new();

        let diagnostic_set =
            diagnostic_set_from_cft(diagnostics, &build.schema.sources, &build.schema.paths);
        for diagnostic in &diagnostic_set {
            let uri = diagnostic
                .primary
                .as_ref()
                .map(|label| lsp_label_location(&label.location))
                .map_or_else(
                    || preferred_diagnostic_uri(&preferred_uris, Path::new("")),
                    |location| label_uri(&location, &preferred_uris),
                );
            by_uri
                .entry(uri)
                .or_default()
                .push(lsp_diagnostic(diagnostic));
        }

        let mut touched_uris = self.published_uris.clone();
        for path in build.schema.paths.values() {
            touched_uris.insert(preferred_diagnostic_uri(&preferred_uris, Path::new(path)));
        }
        for document in self.open_documents.values() {
            touched_uris.insert(document.uri.clone());
        }
        for (uri, diagnostics) in non_schema_diagnostics {
            by_uri.insert(uri.clone(), diagnostics);
            touched_uris.insert(uri);
        }

        let publications = touched_uris
            .into_iter()
            .map(|uri| {
                let diagnostics = by_uri.remove(&uri).unwrap_or_default();
                self.record_publication(uri, diagnostics)
            })
            .collect();

        self.build = Some(build);
        Ok(publications)
    }

    pub(crate) fn ensure_build_publications(
        &mut self,
    ) -> Result<Vec<DiagnosticPublication>, String> {
        if self.build.is_none() {
            return self.validate_project();
        }
        Ok(Vec::new())
    }

    pub(crate) fn prepare_request_document(
        &mut self,
        _uri: &str,
    ) -> Result<Vec<DiagnosticPublication>, String> {
        self.ensure_build_publications()
    }

    pub(crate) fn request_document(&self, uri: &str) -> LspRequestDocument<'_> {
        if let Some(source) = self.cfd_source_by_uri(uri) {
            let (ast, _) = parse_cfd(&source);
            return LspRequestDocument::Cfd(CfdRequestDocument {
                source,
                ast,
                schema: self.schema(),
                build: self.build.as_ref(),
            });
        }
        let Some(build) = self.build.as_ref() else {
            return LspRequestDocument::Missing;
        };
        let Some(document) = build.document_by_uri(uri) else {
            return LspRequestDocument::Missing;
        };
        LspRequestDocument::Cft { build, document }
    }

    pub(crate) fn cfd_project_sources(&self) -> Vec<CfdProjectSource> {
        let mut sources = Vec::new();
        for source in &self.project.config.sources {
            let SourceLocationSpec::Path(path) = source.location() else {
                continue;
            };
            let resolved = self.project.resolve_path(path);
            if resolved.is_dir() {
                sources.extend(cfd_sources_in_dir(&resolved));
            } else if is_cfd_path(&resolved) {
                if let Some(source) = cfd_source_from_path(&resolved) {
                    sources.push(source);
                }
            }
        }
        let mut project_paths = sources
            .iter()
            .map(|source| source.path.clone())
            .collect::<BTreeSet<_>>();
        for source in &mut sources {
            if let Some(document) = self.open_documents.get(&source.path) {
                source.uri.clone_from(&document.uri);
                source.text.clone_from(&document.text);
            }
        }
        for (path, document) in &self.open_documents {
            if is_cfd_path(path) && project_paths.insert(path.clone()) {
                sources.push(CfdProjectSource {
                    path: path.clone(),
                    uri: document.uri.clone(),
                    text: document.text.clone(),
                });
            }
        }
        sources.sort_by(|left, right| left.path.cmp(&right.path));
        sources
    }

    fn cfd_source_by_uri(&self, uri: &str) -> Option<String> {
        let path = path_from_file_uri(uri)?;
        if !is_cfd_path(&path) {
            return None;
        }
        let normalized = normalize_path(&path);
        self.open_documents
            .get(&normalized)
            .map(|document| document.text.clone())
    }

    fn preferred_diagnostic_uris(
        &self,
        schema_by_path: &BTreeMap<PathBuf, (String, PathBuf)>,
    ) -> BTreeMap<PathBuf, String> {
        let mut preferred = BTreeMap::new();
        for (normalized_path, document) in &self.open_documents {
            if schema_by_path.contains_key(normalized_path) {
                preferred.insert(normalized_path.clone(), document.uri.clone());
            }
        }
        preferred
    }

    fn diagnostic_set_publications(
        &mut self,
        diagnostics: &DiagnosticSet,
        preferred_uris: &BTreeMap<PathBuf, String>,
        fallback_path: &Path,
    ) -> Vec<DiagnosticPublication> {
        let mut by_uri: BTreeMap<String, Vec<Value>> = BTreeMap::new();
        for diagnostic in diagnostics {
            let uri = diagnostic
                .primary
                .as_ref()
                .map(|label| lsp_label_location(&label.location))
                .map_or_else(
                    || preferred_diagnostic_uri(preferred_uris, fallback_path),
                    |location| label_uri(&location, preferred_uris),
                );
            by_uri
                .entry(uri)
                .or_default()
                .push(lsp_diagnostic(diagnostic));
        }
        by_uri
            .into_iter()
            .map(|(uri, diagnostics)| self.record_publication(uri, diagnostics))
            .collect()
    }

    fn record_publication(
        &mut self,
        uri: String,
        diagnostics: Vec<Value>,
    ) -> DiagnosticPublication {
        self.published_uris.insert(uri.clone());
        DiagnosticPublication { uri, diagnostics }
    }
}

fn cfd_sources_in_dir(dir: &Path) -> Vec<CfdProjectSource> {
    let mut sources = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return sources;
    };
    let mut entries = entries.filter_map(Result::ok).collect::<Vec<_>>();
    entries.sort_by_key(std::fs::DirEntry::path);
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            sources.extend(cfd_sources_in_dir(&path));
        } else if is_cfd_path(&path) {
            if let Some(source) = cfd_source_from_path(&path) {
                sources.push(source);
            }
        }
    }
    sources
}

fn cfd_source_from_path(path: &Path) -> Option<CfdProjectSource> {
    let text = std::fs::read_to_string(path).ok()?;
    let normalized = normalize_path(path);
    Some(CfdProjectSource {
        uri: path_to_file_uri(&normalized),
        path: normalized,
        text,
    })
}

pub(crate) fn is_cfd_path(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e == "cfd")
}
