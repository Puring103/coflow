//! Project session construction through the shared Coflow engine.

use coflow_api::{ProviderRegistry, WriterCapabilities};
use coflow_runtime::{build_project_session_read_only, FileTreeNode};
use coflow_project::Project;
use std::collections::HashMap;

use super::diagnostics::diagnostics_from_store;
use super::EditorSession;
use crate::editor::types::EditorError;

const FALLBACK_PROVIDER_ID: &str = "unknown";

pub(super) struct SessionSnapshotParts {
    pub(super) file_tree: Vec<FileTreeNode>,
}

pub(super) fn default_provider_registry() -> Result<ProviderRegistry, EditorError> {
    coflow_builtins::default_provider_registry()
        .map_err(|err| EditorError::project(format!("failed to register default providers: {err}")))
}

pub(super) fn session_capabilities_for_file(
    session: &EditorSession,
    registry: &ProviderRegistry,
    file_path: &str,
) -> WriterCapabilities {
    let provider_id = session
        .engine
        .files
        .source_for_display(file_path)
        .and_then(|source_id| session.engine.sources.entries().get(source_id.index()))
        .map_or(FALLBACK_PROVIDER_ID, |entry| entry.provider_id.as_str());
    let writer = registry.source_writer(provider_id);
    writer.map_or_else(
        || WriterCapabilities::read_only().with_provider_id(provider_id),
        |w| {
            let descriptor = w.descriptor();
            descriptor
                .capabilities
                .clone()
                .with_provider_id(descriptor.id)
        },
    )
}

pub(super) fn build_session(
    yaml_path_in: &std::path::Path,
    registry: &ProviderRegistry,
) -> Result<(EditorSession, SessionSnapshotParts), EditorError> {
    let project = Project::open_schema_only(Some(yaml_path_in))
        .map_err(|err| EditorError::project(format!("failed to open project: {err}")))?;
    let yaml_path = project.config_path.clone();
    let project_root = project.root_dir.clone();
    let engine = build_project_session_read_only(project, registry)
        .map_err(|err| EditorError::project(format!("failed to build project: {err}")))?;
    let file_tree = engine.file_tree();
    let diagnostics = diagnostics_from_store(&engine.diagnostics, &project_root);

    Ok((
        EditorSession {
            project_root,
            yaml_path,
            engine,
            diagnostics,
            ref_target_cache: HashMap::new(),
        },
        SessionSnapshotParts { file_tree },
    ))
}
