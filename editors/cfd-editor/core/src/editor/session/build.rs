//! Project session construction through the shared Coflow engine.

use coflow_runtime::Project;
use coflow_runtime::{DiagnosticSet, WriterCapabilities};
use coflow_runtime::{FileTreeNode, ProjectQueries, ProjectRuntime, Runtime};
use std::collections::{BTreeMap, HashMap, HashSet};

use super::diagnostics::diagnostics_from_store;
use super::revision::RevisionCoordinator;
use super::EditorSession;
use crate::editor::types::EditorError;

pub(super) struct SessionSnapshotParts {
    pub(super) file_tree: Vec<FileTreeNode>,
}

type FileTypeNames = BTreeMap<String, Vec<String>>;
type TypeDisplayNames = BTreeMap<(String, String), String>;

pub(super) fn session_capabilities_for_file(
    session: &EditorSession,
    file_path: &str,
) -> WriterCapabilities {
    session.engine.writer_capabilities_for_file(file_path)
}

pub(super) fn build_session(
    yaml_path_in: &std::path::Path,
) -> Result<(EditorSession, SessionSnapshotParts), EditorError> {
    let project = Project::open_schema_only(Some(yaml_path_in)).map_err(|err| {
        EditorError::project(prefixed_diagnostics("failed to open project", &err))
    })?;
    let yaml_path = project.config_path().to_path_buf();
    let project_root = project.root_dir().to_path_buf();
    let schema_files = project
        .schema_sources()
        .map_err(|err| EditorError::project(prefixed_diagnostics("failed to discover schema", &err)))?
        .into_iter()
        .filter_map(|source| source.canonical_path.strip_prefix(&project_root).ok().map(coflow_runtime::path_to_slash))
        .collect();
    let runtime = Runtime::new();
    let language_server = coflow::lsp::EmbeddedLsp::new(project.clone());
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
    let (file_type_names, type_display_names) = type_navigation(engine.queries());
    let diagnostics = diagnostics_from_store(engine.queries().diagnostics(), &project_root);

    Ok((
        EditorSession {
            project_root,
            yaml_path,
            engine,
            diagnostics,
            language_server,
            language_documents: HashSet::new(),
            language_diagnostics: HashMap::new(),
            schema_files,
            file_type_names,
            type_display_names,
            ref_target_cache: HashMap::new(),
            revisions: RevisionCoordinator::initial(),
        },
        SessionSnapshotParts { file_tree },
    ))
}

fn type_navigation(queries: ProjectQueries<'_>) -> (FileTypeNames, TypeDisplayNames) {
    let display_names = BTreeMap::new();
    let mut file_type_names = BTreeMap::new();
    let concrete_types = queries
        .schema_type_names()
        .into_iter()
        .filter(|name| !queries.type_is_abstract(name))
        .collect::<Vec<_>>();
    for file_path in queries.source_files() {
        let mut type_names = concrete_types.clone();
        let mut type_seen = type_names.iter().cloned().collect::<HashSet<_>>();
        for view in queries.record_views_in_file(file_path) {
            let type_name = view.coordinate.actual_type.to_string();
            if type_seen.insert(type_name.clone()) {
                type_names.push(type_name);
            }
        }
        file_type_names.insert(file_path.to_string(), type_names);
    }
    (file_type_names, display_names)
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
