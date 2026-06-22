//! Project loading: open the YAML, compile the schema, run every loader,
//! build the model, run checks, and capture diagnostics + the dependency
//! graph used by future incremental check runs.
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;
use std::sync::Arc;

use coflow_api::{
    DataLoader, LoadContext, ProjectSourceRef, ProviderRegistry, ResolvedSource,
    SourceLocationSpec, SourceResolveContext,
};
use coflow_cft::CftContainer;
use coflow_checker::{run_checks_with_deps, DependencyGraph};
use coflow_data_model::CfdDataModel;
use coflow_project::{compile_schema_project, Project, SourceConfig};

use crate::types::{DiagnosticItem, EditorError, FileTreeNode, SourceCapabilities};

use super::diagnostics::{
    diagnostic_from_api, diagnostic_from_cfd, diagnostic_from_cft_schema, diagnostic_from_project,
    loader_register_diagnostic, Diagnostics,
};
use super::file_tree::build_file_tree;
use super::path::path_to_slash;
use super::EditorSession;

const FALLBACK_PROVIDER_ID: &str = "unknown";

pub(super) struct SessionSnapshotParts {
    pub(super) file_tree: Vec<FileTreeNode>,
}

/// Construct the session's `ProviderRegistry` with every loader and writer
/// the editor knows about. Returns the registry and a list of any
/// registration warnings to surface to the user.
pub(super) fn default_provider_registry() -> (ProviderRegistry, Vec<DiagnosticItem>) {
    let mut registry = ProviderRegistry::default();
    let mut warnings = Vec::new();
    if let Err(err) = registry.register_loader(coflow_loader_cfd::CfdLoader) {
        warnings.push(loader_register_diagnostic(&err));
    }
    if let Err(err) = registry.register_loader(coflow_loader_excel::ExcelLoader) {
        warnings.push(loader_register_diagnostic(&err));
    }
    if let Err(err) = registry.register_loader(coflow_loader_lark::LarkSheetLoader::<
        coflow_loader_lark::UreqLarkHttpClient,
    >::default())
    {
        warnings.push(loader_register_diagnostic(&err));
    }
    if let Err(err) = registry.register_writer(coflow_loader_cfd::CfdWriter::new()) {
        warnings.push(loader_register_diagnostic(&err));
    }
    if let Err(err) = registry.register_writer(coflow_loader_excel::ExcelWriter::new()) {
        warnings.push(loader_register_diagnostic(&err));
    }
    if let Err(err) =
        registry.register_writer(coflow_loader_lark::LarkSheetWriter::<
            coflow_loader_lark::UreqLarkHttpClient,
        >::default())
    {
        warnings.push(loader_register_diagnostic(&err));
    }
    (registry, warnings)
}

pub(super) fn session_capabilities_for_file(
    session: &EditorSession,
    registry: &ProviderRegistry,
    file_path: &str,
) -> SourceCapabilities {
    let provider_id = session
        .source_for_file
        .get(file_path)
        .map(|s| s.provider_id.as_str())
        .unwrap_or(FALLBACK_PROVIDER_ID);
    let writer = registry.writer(provider_id);
    match writer {
        Some(w) => {
            let descriptor = w.descriptor();
            SourceCapabilities::from_writer(descriptor.id, descriptor.capabilities)
        }
        None => SourceCapabilities::read_only(static_provider_id(provider_id)),
    }
}

fn static_provider_id(id: &str) -> &'static str {
    match id {
        "cfd" => "cfd",
        "excel" => "excel",
        "lark-sheet" => "lark-sheet",
        _ => "unknown",
    }
}

#[allow(clippy::too_many_arguments)]
fn load_one_source(
    project: &Project,
    schema: &CftContainer,
    registry: &ProviderRegistry,
    source: &SourceConfig,
    source_files: &mut BTreeSet<String>,
    model_builder: &mut coflow_data_model::CfdModelBuilder<'_>,
    file_to_keys: &mut BTreeMap<String, Vec<String>>,
    key_to_file: &mut HashMap<String, String>,
    source_for_file: &mut HashMap<String, ResolvedSource>,
    diagnostics: &mut Vec<DiagnosticItem>,
) {
    let configured = configured_resolved_source(project, source);
    let resolve_ctx = SourceResolveContext {
        project_root: &project.root_dir,
        schema,
    };

    type Pair = (Arc<dyn DataLoader>, ResolvedSource);
    let mut pairs: Vec<Pair> = Vec::new();
    let is_untyped_dir = source.source_type.is_none()
        && matches!(&configured.location, SourceLocationSpec::Path(p) if p.is_dir());

    if is_untyped_dir {
        for loader in registry.loaders() {
            match loader.resolve(resolve_ctx, &configured) {
                Ok(subs) => {
                    for sub in subs {
                        pairs.push((Arc::clone(&loader), sub));
                    }
                }
                Err(diag_set) => {
                    for d in diag_set.diagnostics {
                        diagnostics.push(diagnostic_from_api(&d));
                    }
                }
            }
        }
    } else {
        let option_keys: Vec<&str> = configured
            .options
            .as_object()
            .map(|m| m.keys().map(String::as_str).collect())
            .unwrap_or_default();
        let source_ref = ProjectSourceRef {
            source_type: source.source_type.as_deref(),
            location: &configured.location,
            option_keys: &option_keys,
        };
        let loader = match registry.select_loader(&source_ref) {
            Ok(loader) => loader,
            Err(err) => {
                diagnostics.push(DiagnosticItem {
                    severity: "error".to_string(),
                    code: "PROJECT-001".to_string(),
                    stage: "PROJECT".to_string(),
                    message: format!(
                        "source `{}` could not select a loader: {err:?}",
                        configured.display_name
                    ),
                    file_path: None,
                    record_key: None,
                    field_path: None,
                });
                return;
            }
        };
        match loader.resolve(resolve_ctx, &configured) {
            Ok(subs) => {
                for sub in subs {
                    pairs.push((Arc::clone(&loader), sub));
                }
            }
            Err(diag_set) => {
                for d in diag_set.diagnostics {
                    diagnostics.push(diagnostic_from_api(&d));
                }
                return;
            }
        }
    }

    let load_ctx = LoadContext {
        project_root: &project.root_dir,
        schema,
    };

    for (loader, sub) in &pairs {
        let mut effective_source = sub.clone();
        if effective_source.provider_id.is_empty() {
            effective_source.provider_id = loader.descriptor().id.to_string();
        }
        let label = file_label_for(project, &effective_source);
        source_files.insert(label.clone());
        source_for_file.insert(label.clone(), effective_source.clone());

        match loader.load(load_ctx, &effective_source) {
            Ok(loaded) => {
                let entry = file_to_keys.entry(label.clone()).or_default();
                for record in loaded.records {
                    let key = record.key.clone();
                    entry.push(key.clone());
                    key_to_file.insert(key, label.clone());
                    model_builder.add_input_record(record);
                }
            }
            Err(diag_set) => {
                for d in diag_set.diagnostics {
                    let mut item = diagnostic_from_api(&d);
                    if item.file_path.is_none() {
                        item.file_path = Some(label.clone());
                    }
                    diagnostics.push(item);
                }
            }
        }
    }
}

fn configured_resolved_source(project: &Project, source: &SourceConfig) -> ResolvedSource {
    let location = match source.location() {
        SourceLocationSpec::Path(path) => SourceLocationSpec::Path(project.resolve_path(path)),
        SourceLocationSpec::Uri(uri) => SourceLocationSpec::Uri(uri.clone()),
    };
    let display_name = match source.location() {
        SourceLocationSpec::Path(path) => path.display().to_string(),
        SourceLocationSpec::Uri(uri) => uri.clone(),
    };
    ResolvedSource {
        provider_id: source.source_type.clone().unwrap_or_default(),
        location,
        options: source.options().clone(),
        display_name,
    }
}

fn file_label_for(project: &Project, sub: &ResolvedSource) -> String {
    match &sub.location {
        SourceLocationSpec::Path(p) => {
            let rel = p.strip_prefix(&project.root_dir).unwrap_or(p.as_path());
            path_to_slash(rel)
        }
        SourceLocationSpec::Uri(uri) => uri.clone(),
    }
}

pub(super) fn build_session(
    yaml_path_in: &Path,
    registry: &ProviderRegistry,
) -> Result<(EditorSession, SessionSnapshotParts), EditorError> {
    let yaml_path = yaml_path_in.to_path_buf();
    let mut diagnostics = Diagnostics::default();

    let project = Project::open_schema_only(Some(yaml_path.as_path())).map_err(|err| {
        EditorError::project(format!("failed to open project: {err}"))
    })?;
    let project_root = project.root_dir.clone();

    for d in project.schema_diagnostics() {
        diagnostics.schema.push(diagnostic_from_project(&d));
    }
    for d in project.data_diagnostics() {
        diagnostics.load.push(diagnostic_from_project(&d));
    }

    let schema = match compile_schema_project(&project, None) {
        Ok(build) => {
            for diag in build.diagnostics {
                diagnostics.schema.push(diagnostic_from_cft_schema(&diag));
            }
            build.container.unwrap_or_else(CftContainer::new)
        }
        Err(err) => {
            diagnostics.schema.push(DiagnosticItem {
                severity: "error".to_string(),
                code: "SCHEMA-COMPILE".to_string(),
                stage: "SCHEMA".to_string(),
                message: err,
                file_path: None,
                record_key: None,
                field_path: None,
            });
            CftContainer::new()
        }
    };

    let mut source_files: BTreeSet<String> = BTreeSet::new();
    let mut model_builder = CfdDataModel::builder(&schema);
    let mut file_to_keys: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut key_to_file: HashMap<String, String> = HashMap::new();
    let mut source_for_file: HashMap<String, ResolvedSource> = HashMap::new();
    let mut load_diagnostics: Vec<DiagnosticItem> = Vec::new();

    for source in &project.config.sources {
        load_one_source(
            &project,
            &schema,
            registry,
            source,
            &mut source_files,
            &mut model_builder,
            &mut file_to_keys,
            &mut key_to_file,
            &mut source_for_file,
            &mut load_diagnostics,
        );
    }
    diagnostics.load.extend(load_diagnostics);

    // We need the input records' origins for diagnostic mapping if the
    // builder fails. `model_builder` consumed them, so re-derive a slice via
    // the builder's `records` accessor (private). Until that lands, we
    // accept that builder errors won't have origin-mapped labels — they
    // already include the record id and key in the message body.
    let (model, check_deps) = match model_builder.build() {
        Ok(model) => {
            let (result, deps) = run_checks_with_deps(&schema, &model);
            if let Err(diags) = result {
                for diag in diags.diagnostics {
                    diagnostics
                        .check
                        .push(diagnostic_from_cfd(&diag, &model, &key_to_file));
                }
            }
            (model, deps)
        }
        Err(diags) => {
            for diag in diags.diagnostics {
                diagnostics.load.push(DiagnosticItem {
                    severity: "error".to_string(),
                    code: diag.code.as_str().to_string(),
                    stage: diag.stage.to_string(),
                    message: diag.message,
                    file_path: None,
                    record_key: None,
                    field_path: None,
                });
            }
            let empty_schema = CftContainer::new();
            let empty_model = CfdDataModel::builder(&empty_schema)
                .build()
                .unwrap_or_else(|_| panic!("empty model build failed"));
            (empty_model, DependencyGraph::default())
        }
    };

    let mut ext_whitelist: BTreeSet<String> = BTreeSet::new();
    for loader in registry.loaders() {
        for ext in loader.descriptor().extensions {
            ext_whitelist.insert((*ext).to_string());
        }
    }

    let file_tree = build_file_tree(&project_root, &source_files, &ext_whitelist);

    Ok((
        EditorSession {
            project_root,
            yaml_path,
            schema,
            model,
            diagnostics,
            check_deps,
            source_files,
            key_to_file,
            file_to_keys,
            source_for_file,
        },
        SessionSnapshotParts { file_tree },
    ))
}
