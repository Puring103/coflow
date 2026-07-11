use coflow_api::{DiagnosticSet, ProviderRegistry};

use crate::session_build::{open_project_session, SessionOpenOptions};
use crate::ProjectSession;

pub(super) fn rebuild_session_after_write(
    session: &ProjectSession,
    registry: &ProviderRegistry,
) -> Result<ProjectSession, DiagnosticSet> {
    open_project_session(
        session.project.clone(),
        registry,
        SessionOpenOptions::build(),
    )
}
