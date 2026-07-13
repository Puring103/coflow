use coflow_api::{DiagnosticSet, ProviderRegistry};
use std::collections::BTreeSet;

use crate::session_build::rebuild_project_session_from_generation;
use crate::ProjectSession;

pub(crate) struct MutationRebuild {
    pub(crate) session: ProjectSession,
    pub(crate) changed_dimension_files: Vec<String>,
}

pub(super) fn rebuild_session_after_write(
    session: &ProjectSession,
    registry: &ProviderRegistry,
    affected_files: &BTreeSet<String>,
) -> Result<MutationRebuild, DiagnosticSet> {
    let output =
        rebuild_project_session_from_generation(session, registry, affected_files)?;
    let changed_dimension_files = output
        .changed_dimension_paths
        .iter()
        .map(|path| {
            path.strip_prefix(&session.project.root_dir).map_or_else(
                |_| path.display().to_string(),
                coflow_project::path_to_slash,
            )
        })
        .collect();
    Ok(MutationRebuild {
        session: output.session,
        changed_dimension_files,
    })
}
