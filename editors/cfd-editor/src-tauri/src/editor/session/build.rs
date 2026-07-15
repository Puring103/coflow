//! Project session construction through the shared Coflow engine.

use coflow_api::{DiagnosticSet, ProviderRegistry, WriterCapabilities};
use coflow_project::Project;
use coflow_runtime::{FileTreeNode, ProjectRuntime, Runtime};
use std::collections::HashMap;

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
    let diagnostics = diagnostics_from_store(engine.queries().diagnostics(), &project_root);

    Ok((
        EditorSession {
            project_root,
            yaml_path,
            engine,
            diagnostics,
            ref_target_cache: HashMap::new(),
            revisions: RevisionCoordinator::initial(),
        },
        SessionSnapshotParts { file_tree },
    ))
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
