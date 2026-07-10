use coflow_api::{DiagnosticSet, ProviderRegistry};
use coflow_project::Project;

use crate::schema_build::build_project_schema_session;
use crate::session::{ProjectSchemaSession, ProjectSession};
use crate::session_build::{build_project_session_for_build, open_project_session_read_only};

#[derive(Debug, Clone)]
pub struct Runtime {
    registry: ProviderRegistry,
}

impl Runtime {
    #[must_use]
    pub fn new(registry: ProviderRegistry) -> Self {
        Self { registry }
    }

    #[must_use]
    pub const fn registry(&self) -> &ProviderRegistry {
        &self.registry
    }

    /// Builds a schema-only session without loading project data.
    ///
    /// # Errors
    ///
    /// Returns unrecoverable project/config/schema I/O diagnostics.
    pub fn build_schema_session(project: Project) -> Result<ProjectSchemaSession, DiagnosticSet> {
        build_project_schema_session(project)
    }

    /// Opens data for editor, inspection, and background tasks that must not
    /// write generated dimension sources.
    ///
    /// # Errors
    ///
    /// Returns unrecoverable project/config/schema I/O diagnostics.
    pub fn open_read_only_session(
        &self,
        project: Project,
    ) -> Result<ReadOnlyProjectSession, DiagnosticSet> {
        open_project_session_read_only(project, &self.registry).map(ReadOnlyProjectSession::new)
    }

    /// Builds data for the normal build pipeline. This may write generated
    /// dimension sources before the final reload.
    ///
    /// # Errors
    ///
    /// Returns unrecoverable project/config/schema I/O diagnostics.
    pub fn build_project_session(
        &self,
        project: Project,
    ) -> Result<BuildProjectSession, DiagnosticSet> {
        build_project_session_for_build(project, &self.registry).map(BuildProjectSession::new)
    }
}

#[derive(Debug)]
pub struct ReadOnlyProjectSession {
    session: ProjectSession,
}

impl ReadOnlyProjectSession {
    #[must_use]
    pub fn new(session: ProjectSession) -> Self {
        Self { session }
    }

    #[must_use]
    pub const fn as_session(&self) -> &ProjectSession {
        &self.session
    }

    #[must_use]
    pub fn into_session(self) -> ProjectSession {
        self.session
    }
}

#[derive(Debug)]
pub struct BuildProjectSession {
    session: ProjectSession,
}

impl BuildProjectSession {
    #[must_use]
    pub fn new(session: ProjectSession) -> Self {
        Self { session }
    }

    #[must_use]
    pub const fn as_session(&self) -> &ProjectSession {
        &self.session
    }

    #[must_use]
    pub fn into_session(self) -> ProjectSession {
        self.session
    }
}
