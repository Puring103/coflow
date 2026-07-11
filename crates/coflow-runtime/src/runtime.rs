use std::collections::BTreeMap;

use coflow_api::{DiagnosticSet, ProviderRegistry};
use coflow_data_model::{CfdPathSegment, CfdValue};
use coflow_project::Project;

use crate::schema_build::build_project_schema_session;
use crate::session::{ProjectSchemaSession, ProjectSession};
use crate::session_build::{open_project_session, SessionOpenOptions};
use crate::{ProjectQueries, WriteOutcome};

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
        open_project_session(project, &self.registry, SessionOpenOptions::read_only())
            .map(ReadOnlyProjectSession::new)
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
        open_project_session(project, &self.registry, SessionOpenOptions::build())
            .map(BuildProjectSession::new)
    }

    /// Opens a mutation-capable session without generating dimension files.
    /// The session owns the registry used by every command and rebuild.
    ///
    /// # Errors
    ///
    /// Returns unrecoverable project/config/schema I/O diagnostics.
    pub fn open_write_session(
        &self,
        project: Project,
    ) -> Result<WriteProjectSession, DiagnosticSet> {
        open_project_session(project, &self.registry, SessionOpenOptions::read_only())
            .map(|session| WriteProjectSession::new(session, self.registry.clone()))
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

    #[must_use]
    pub const fn queries(&self) -> ProjectQueries<'_> {
        ProjectQueries::new(&self.session, 0)
    }

    #[must_use]
    pub fn into_diagnostics(self) -> DiagnosticSet {
        self.session.into_diagnostics()
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

    #[must_use]
    pub const fn queries(&self) -> ProjectQueries<'_> {
        ProjectQueries::new(&self.session, 0)
    }

    #[must_use]
    pub fn into_diagnostics(self) -> DiagnosticSet {
        self.session.into_diagnostics()
    }
}

#[derive(Debug)]
pub struct WriteProjectSession {
    session: ProjectSession,
    registry: ProviderRegistry,
    revision: u64,
}

impl WriteProjectSession {
    fn new(session: ProjectSession, registry: ProviderRegistry) -> Self {
        Self {
            session,
            registry,
            revision: 0,
        }
    }

    #[must_use]
    pub const fn queries(&self) -> ProjectQueries<'_> {
        ProjectQueries::new(&self.session, self.revision)
    }

    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    pub fn write_field(
        &mut self,
        actual_type: &str,
        key: &str,
        path: &[CfdPathSegment],
        new_value: &CfdValue,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        let outcome = self
            .session
            .write_field(&self.registry, actual_type, key, path, new_value)?;
        self.revision = self.revision.saturating_add(1);
        Ok(outcome)
    }

    pub fn rename_record_key(
        &mut self,
        actual_type: &str,
        old_key: &str,
        new_key: &str,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        let outcome = self.session.rename_record_key(
            &self.registry,
            actual_type,
            old_key,
            new_key,
        )?;
        self.revision = self.revision.saturating_add(1);
        Ok(outcome)
    }

    pub fn insert_record(
        &mut self,
        file: &str,
        sheet: Option<&str>,
        record_key: &str,
        actual_type: &str,
        fields: &BTreeMap<String, CfdValue>,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        let outcome = self.session.insert_record(
            &self.registry,
            file,
            sheet,
            record_key,
            actual_type,
            fields,
        )?;
        self.revision = self.revision.saturating_add(1);
        Ok(outcome)
    }

    pub fn delete_record(
        &mut self,
        actual_type: &str,
        key: &str,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        let outcome = self
            .session
            .delete_record(&self.registry, actual_type, key)?;
        self.revision = self.revision.saturating_add(1);
        Ok(outcome)
    }
}
