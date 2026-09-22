//! Project session construction through the shared Coflow engine.

use coflow_project::Project;
use coflow_project::{DiagnosticSet, WriterCapabilities};
use coflow_project::{FileTreeNode, ProjectQueries, ProjectRuntime, Runtime};
use std::collections::{BTreeMap, HashMap, HashSet};

use super::diagnostics::diagnostics_from_store;
use super::revision::RevisionCoordinator;
use super::EditorSession;
use crate::editor::types::EditorError;

pub(crate) struct SessionSnapshotParts {
    pub(crate) file_tree: Vec<FileTreeNode>,
}

type SchemaTypeNames = Vec<String>;
type FileTypeCounts = BTreeMap<String, BTreeMap<String, usize>>;

pub(crate) fn session_capabilities_for_file(
    session: &EditorSession,
    file_path: &str,
) -> WriterCapabilities {
    session.engine.writer_capabilities_for_file(file_path)
}

pub(crate) fn build_session(
    yaml_path_in: &std::path::Path,
) -> Result<(EditorSession, SessionSnapshotParts), EditorError> {
    let project = Project::open_schema_only(Some(yaml_path_in)).map_err(|err| {
        EditorError::project(prefixed_diagnostics("failed to open project", &err))
    })?;
    let yaml_path = project.config_path().to_path_buf();
    let project_root = project.root_dir().to_path_buf();
    let schema_files = project
        .schema_sources()
        .map_err(|err| {
            EditorError::project(prefixed_diagnostics("failed to discover schema", &err))
        })?
        .into_iter()
        .map(|source| source.module_id)
        .collect();
    let runtime = Runtime::new();
    let mut schema_runtime = ProjectRuntime::new(project.clone());
    let _ = schema_runtime.refresh();
    let schema_session = schema_runtime
        .latest_attempt()
        .cloned()
        .ok_or_else(|| EditorError::project("failed to build project schema".to_string()))?;
    let engine = runtime
        .open_write_session_from_schema(schema_session)
        .map_err(|err| {
            EditorError::project(prefixed_diagnostics("failed to build project", &err))
        })?;
    let language_server = coflow_lsp::EmbeddedLsp::with_schema_runtime(project, schema_runtime);
    let file_tree = engine.queries().file_tree();
    let (schema_type_names, file_type_counts) = type_navigation(engine.queries());
    let diagnostics = diagnostics_from_store(engine.queries(), &project_root);

    Ok((
        EditorSession {
            project_root,
            yaml_path,
            engine,
            schema_revision: 1,
            diagnostics,
            language_server,
            language_documents: HashSet::new(),
            language_diagnostics: HashMap::new(),
            schema_files,
            schema_type_names,
            file_type_counts,
            ref_target_cache: HashMap::new(),
            shape_cache: crate::editor::convert::ShapeCache::default(),
            revisions: RevisionCoordinator::initial(),
        },
        SessionSnapshotParts { file_tree },
    ))
}

/// schema 类型目录由会话共享，记录计数按文件独立维护。
pub(super) fn type_navigation(queries: ProjectQueries<'_>) -> (SchemaTypeNames, FileTypeCounts) {
    let concrete_types = queries
        .schema_type_names()
        .into_iter()
        .filter(|name| !queries.type_is_abstract(name))
        .collect();
    let counts = queries
        .source_files()
        .map(|file| (file.to_string(), file_type_counts(queries, file)))
        .collect();
    (concrete_types, counts)
}

pub(super) fn file_type_counts(queries: ProjectQueries<'_>, file: &str) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for view in queries.record_views_in_file(file) {
        *counts
            .entry(view.coordinate.actual_type.to_string())
            .or_default() += 1;
    }
    counts
}

pub(crate) fn diagnostic_messages(diagnostics: &DiagnosticSet) -> String {
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
