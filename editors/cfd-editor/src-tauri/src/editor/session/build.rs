//! Project session construction through the shared Coflow engine.

use coflow_api::{DiagnosticSet, ProviderRegistry, WriterCapabilities};
use coflow_project::Project;
use coflow_runtime::{FileTreeNode, ProjectQueries, ProjectRuntime, Runtime};
use std::collections::{BTreeMap, HashMap, HashSet};

use super::diagnostics::diagnostics_from_store;
use super::revision::RevisionCoordinator;
use super::EditorSession;
use crate::editor::types::EditorError;

pub(super) struct SessionSnapshotParts {
    pub(super) file_tree: Vec<FileTreeNode>,
}

pub(super) fn default_provider_registry() -> Result<ProviderRegistry, EditorError> {
    coflow_builtins::default_provider_registry()
        .map_err(|err| EditorError::project(format!("failed to register default providers: {err}")))
}

pub(super) fn session_capabilities_for_file(
    session: &EditorSession,
    file_path: &str,
) -> WriterCapabilities {
    session.engine.writer_capabilities_for_file(file_path)
}

pub(super) fn build_session(
    yaml_path_in: &std::path::Path,
    registry: &ProviderRegistry,
) -> Result<(EditorSession, SessionSnapshotParts), EditorError> {
    let project = Project::open_schema_only(Some(yaml_path_in)).map_err(|err| {
        EditorError::project(prefixed_diagnostics("failed to open project", &err))
    })?;
    let yaml_path = project.config_path.clone();
    let project_root = project.root_dir.clone();
    let runtime = Runtime::new(registry.clone());
    let mut schema_runtime = ProjectRuntime::new(project);
    let _ = schema_runtime.refresh();
    let schema_session = schema_runtime
        .into_latest_attempt()
        .ok_or_else(|| EditorError::project("failed to build project schema".to_string()))?;
    let engine = runtime
        .open_write_session_from_schema(schema_session)
        .map_err(|err| {
            EditorError::project(prefixed_diagnostics("failed to build project", &err))
        })?;
    let file_tree = engine.queries().file_tree();
    let (file_type_names, type_display_names) = type_navigation(engine.queries(), registry, &file_tree);
    let diagnostics = diagnostics_from_store(engine.queries().diagnostics(), &project_root);

    Ok((
        EditorSession {
            project_root,
            yaml_path,
            engine,
            diagnostics,
            file_type_names,
            type_display_names,
            ref_target_cache: HashMap::new(),
            revisions: RevisionCoordinator::initial(),
        },
        SessionSnapshotParts { file_tree },
    ))
}

fn type_navigation(
    queries: ProjectQueries<'_>,
    registry: &ProviderRegistry,
    file_tree: &[FileTreeNode],
) -> (
    BTreeMap<String, Vec<String>>,
    BTreeMap<(String, String), String>,
) {
    let mut files = Vec::new();
    collect_source_files(file_tree, &mut files);
    let mut display_names = BTreeMap::new();
    let mut file_type_names = BTreeMap::new();
    let schema_type_names = queries.schema_type_names();
    for file_path in files {
        let mut type_names = Vec::new();
        let mut type_seen = HashSet::new();
        for view in queries.record_views_in_file(&file_path) {
            let type_name = view.coordinate.actual_type.clone();
            if type_seen.insert(type_name.clone()) {
                type_names.push(type_name);
            }
        }
        for type_name in &schema_type_names {
            let Ok(Some(sheet)) = queries.table_sheet_for_type(registry, &file_path, &type_name)
            else {
                continue;
            };
            if type_seen.insert(type_name.clone()) {
                type_names.push(type_name.clone());
            }
            if sheet != *type_name {
                display_names.insert((file_path.clone(), type_name.clone()), sheet);
            }
        }
        file_type_names.insert(file_path, type_names);
    }
    (file_type_names, display_names)
}

fn collect_source_files(nodes: &[FileTreeNode], files: &mut Vec<String>) {
    for node in nodes {
        if node.is_dir {
            collect_source_files(&node.children, files);
        } else if node.in_sources {
            files.push(node.path.clone());
        }
    }
}

pub(super) fn diagnostic_messages(diagnostics: &DiagnosticSet) -> String {
    diagnostics
        .diagnostics
        .iter()
        .map(|diagnostic| format!("[{}] {}", diagnostic.code, diagnostic.message))
        .collect::<Vec<_>>()
        .join("\n")
}

fn prefixed_diagnostics(prefix: &str, diagnostics: &DiagnosticSet) -> String {
    let messages = diagnostic_messages(diagnostics);
    if messages.is_empty() {
        prefix.to_string()
    } else {
        format!("{prefix}: {messages}")
    }
}
