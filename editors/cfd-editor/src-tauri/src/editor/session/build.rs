//! Project session construction through the shared Coflow engine.

use std::collections::BTreeSet;

use coflow_api::ProviderRegistry;
use coflow_engine::{build_project_session, FileTreeNode, ProjectSession};
use coflow_project::Project;

use super::diagnostics::diagnostics_from_store;
use super::EditorSession;
use crate::editor::types::{EditorError, SourceCapabilities};

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
) -> SourceCapabilities {
    let provider_id = session
        .engine
        .files
        .source_for_display(file_path)
        .and_then(|source_id| session.engine.sources.entries().get(source_id.index()))
        .map_or(FALLBACK_PROVIDER_ID, |entry| entry.provider_id.as_str());
    let writer = registry.writer(provider_id);
    writer.map_or_else(
        || SourceCapabilities::read_only(provider_id),
        |w| {
            let descriptor = w.descriptor();
            SourceCapabilities::from_writer(descriptor.id, descriptor.capabilities)
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
    let engine = build_project_session(project, registry)
        .map_err(|err| EditorError::project(format!("failed to build project: {err}")))?;
    let file_tree = session_file_tree(&engine, registry);
    let diagnostics = diagnostics_from_store(&engine.diagnostics);

    Ok((
        EditorSession {
            project_root,
            yaml_path,
            engine,
            diagnostics,
        },
        SessionSnapshotParts { file_tree },
    ))
}

/// Build the file-tree snapshot the front-end sees. Walks every loader-
/// registered extension via the engine's [`ProjectSession::file_tree`] so
/// other hosts (CLI, future LSP UI) can render the same tree without
/// reimplementing the walker.
fn session_file_tree(engine: &ProjectSession, registry: &ProviderRegistry) -> Vec<FileTreeNode> {
    let mut ext_whitelist: BTreeSet<String> = BTreeSet::new();
    for loader in registry.loaders() {
        for ext in loader.descriptor().extensions {
            ext_whitelist.insert((*ext).to_string());
        }
    }
    engine.file_tree(ext_whitelist)
}
