use super::{BuildProjectSession, ReadOnlyProjectSession, WriteProjectSession};
use crate::api::DiagnosticSet;
use crate::project_schema::open_project_schema_session;
use crate::session::ProjectSchemaSession;
use crate::session_build::{
    open_project_session, open_project_session_from_schema,
    open_project_session_with_source_overrides,
};
use crate::DataSourceTextOverride;
use crate::Project;

/// 无状态的项目会话入口；写入工作区由每次 mutation 独立创建。
#[derive(Debug, Clone)]
pub struct ProjectSessionFactory;

impl ProjectSessionFactory {
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Builds a schema-only session without loading project data.
    ///
    /// # Errors
    ///
    /// Returns unrecoverable project/config/schema I/O diagnostics.
    pub fn open_schema_session(project: Project) -> Result<ProjectSchemaSession, DiagnosticSet> {
        open_project_schema_session(project)
    }

    /// Opens read-only data for editor, inspection, and background tasks.
    ///
    /// # Errors
    ///
    /// Returns unrecoverable project/config/schema I/O diagnostics.
    pub fn open_read_only_session(
        &self,
        project: Project,
    ) -> Result<ReadOnlyProjectSession, DiagnosticSet> {
        open_project_session(project).map(ReadOnlyProjectSession::new)
    }

    /// Opens read-only data using host-provided text for selected source files.
    ///
    /// # Errors
    ///
    /// Returns unrecoverable project/config/schema I/O diagnostics.
    pub fn open_read_only_session_with_source_overrides(
        &self,
        project: Project,
        source_overrides: &[DataSourceTextOverride],
    ) -> Result<ReadOnlyProjectSession, DiagnosticSet> {
        open_project_session_with_source_overrides(project, source_overrides)
            .map(ReadOnlyProjectSession::new)
    }

    /// Builds data for the normal build pipeline without publishing artifacts.
    ///
    /// # Errors
    ///
    /// Returns unrecoverable project/config/schema I/O diagnostics.
    pub fn build_project_session(
        &self,
        project: Project,
    ) -> Result<BuildProjectSession, DiagnosticSet> {
        open_project_session(project).map(BuildProjectSession::new)
    }

    /// Opens a mutation-capable session over the configured business CFD files.
    /// Each mutation creates its own staged write workspace.
    ///
    /// # Errors
    ///
    /// Returns unrecoverable project/config/schema I/O diagnostics.
    pub fn open_write_session(
        &self,
        project: Project,
    ) -> Result<WriteProjectSession, DiagnosticSet> {
        open_project_session(project).map(WriteProjectSession::new)
    }

    /// Opens a write-capable data session from a cached schema generation.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when project sources cannot be opened against the schema.
    pub fn open_write_session_from_schema(
        &self,
        schema: ProjectSchemaSession,
    ) -> Result<WriteProjectSession, DiagnosticSet> {
        open_project_session_from_schema(schema).map(WriteProjectSession::new)
    }

    /// Opens a mutation-capable candidate using host-provided text for
    /// selected data files. No project file is modified.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when the candidate project cannot be loaded.
    pub fn open_write_session_with_source_overrides(
        &self,
        project: Project,
        source_overrides: &[DataSourceTextOverride],
    ) -> Result<WriteProjectSession, DiagnosticSet> {
        open_project_session_with_source_overrides(project, source_overrides)
            .map(WriteProjectSession::new)
    }
}

impl Default for ProjectSessionFactory {
    fn default() -> Self {
        Self::new()
    }
}
