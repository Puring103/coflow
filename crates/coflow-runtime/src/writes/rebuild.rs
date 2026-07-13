use coflow_api::{DiagnosticSet, ProviderRegistry};

use crate::session_build::{build_project_session_with_effects, SessionOpenOptions};
use crate::ProjectSession;

pub(crate) struct MutationRebuild {
    pub(crate) session: ProjectSession,
    pub(crate) changed_dimension_files: Vec<String>,
}

pub(super) fn rebuild_session_after_write(
    session: &ProjectSession,
    registry: &ProviderRegistry,
) -> Result<MutationRebuild, DiagnosticSet> {
    let output = build_project_session_with_effects(
        session.project.clone(),
        registry,
        SessionOpenOptions::build(),
    )?;
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
