use std::collections::BTreeMap;

use coflow_api::{DiagnosticSet, ProviderRegistry, WriterCapabilities};
use coflow_data_model::{CfdPathSegment, CfdValue};
use coflow_project::Project;

use crate::schema_build::build_project_schema_session;
use crate::session::{ProjectSchemaSession, ProjectSession};
use crate::session_build::{open_project_session, SessionOpenOptions};
use crate::{
    CreateRecordDraft, DefaultMaterialization, MutationFields, MutationOp, MutationReport,
    MutationRequest, MutationValue, ProjectQueries, RecordCoordinate, WriteOutcome,
};

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

/// Read capability for a built project.
///
/// The owning runtime session is intentionally sealed. Hosts can query this
/// capability but cannot unwrap it or reach mutation methods.
///
/// ```compile_fail
/// fn escape_session(session: coflow_runtime::ReadOnlyProjectSession) {
///     let _ = session.into_session();
/// }
/// ```
#[derive(Debug)]
pub struct ReadOnlyProjectSession {
    session: ProjectSession,
}

impl ReadOnlyProjectSession {
    fn new(session: ProjectSession) -> Self {
        Self { session }
    }

    #[must_use]
    pub const fn queries(&self) -> ProjectQueries<'_> {
        ProjectQueries::new(&self.session, 0)
    }

    #[must_use]
    pub fn into_diagnostics(self) -> DiagnosticSet {
        self.session.into_diagnostics()
    }

    #[must_use]
    pub fn into_schema_session(self) -> ProjectSchemaSession {
        self.session.into_schema_session()
    }
}

#[derive(Debug)]
pub struct BuildProjectSession {
    session: ProjectSession,
}

impl BuildProjectSession {
    fn new(session: ProjectSession) -> Self {
        Self { session }
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

    #[must_use]
    pub fn writer_capabilities_for_file(&self, file: &str) -> WriterCapabilities {
        self.queries()
            .writer_capabilities_for_file(&self.registry, file)
    }

    /// Build a schema-shaped default record value.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when `type_name` is unknown.
    pub fn default_record_value(
        &self,
        type_name: &str,
        materialization: DefaultMaterialization,
    ) -> Result<CfdValue, DiagnosticSet> {
        self.session
            .default_record_value(type_name, materialization)
    }

    /// Build the editable fields needed to insert a record.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when the type cannot be inserted.
    pub fn create_record_draft(
        &self,
        type_name: &str,
    ) -> Result<CreateRecordDraft, DiagnosticSet> {
        self.session.create_record_draft(type_name)
    }

    /// Build a default collection item for an editor insertion.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when the path is not a collection or no valid
    /// reference target exists.
    pub fn default_collection_item_value(
        &self,
        actual_type: &str,
        path: &[CfdPathSegment],
    ) -> Result<CfdValue, DiagnosticSet> {
        self.session
            .default_collection_item_value(actual_type, path)
    }

    /// Apply a batch of mutation commands using the registry owned by this
    /// capability.
    ///
    /// # Errors
    ///
    /// Returns diagnostics only when mutation execution cannot produce a
    /// report. Per-operation failures are included in the report.
    pub fn apply_mutation(
        &mut self,
        request: MutationRequest,
    ) -> Result<MutationReport, DiagnosticSet> {
        let report = self.session.apply_mutation(&self.registry, request)?;
        if !report.applied.is_empty() {
            self.revision = self.revision.saturating_add(1);
        }
        Ok(report)
    }

    pub fn write_field(
        &mut self,
        actual_type: &str,
        key: &str,
        path: &[CfdPathSegment],
        new_value: &CfdValue,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        self.apply_one(MutationOp::SetField {
            record: RecordCoordinate::new(actual_type, key),
            file: None,
            path: path.to_vec(),
            value: MutationValue::Cfd(new_value.clone()),
        })
    }

    pub fn rename_record_key(
        &mut self,
        actual_type: &str,
        old_key: &str,
        new_key: &str,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        self.apply_one(MutationOp::RenameRecord {
            record: RecordCoordinate::new(actual_type, old_key),
            file: None,
            new_key: new_key.to_string(),
        })
    }

    pub fn insert_record(
        &mut self,
        file: &str,
        sheet: Option<&str>,
        record_key: &str,
        actual_type: &str,
        fields: &BTreeMap<String, CfdValue>,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        self.apply_one(MutationOp::InsertRecord {
            file: file.to_string(),
            sheet: sheet.map(ToOwned::to_owned),
            actual_type: actual_type.to_string(),
            key: record_key.to_string(),
            fields: MutationFields::Cfd(fields.clone()),
            materialization: DefaultMaterialization::Minimal,
        })
    }

    pub fn delete_record(
        &mut self,
        actual_type: &str,
        key: &str,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        self.apply_one(MutationOp::DeleteRecord {
            record: RecordCoordinate::new(actual_type, key),
            file: None,
        })
    }

    fn apply_one(&mut self, op: MutationOp) -> Result<WriteOutcome, DiagnosticSet> {
        let report = self.apply_mutation(MutationRequest {
            stop_on_write_error: true,
            ops: vec![op],
        })?;
        if let Some(applied) = report.applied.into_iter().next() {
            return Ok(applied.outcome);
        }
        let mut diagnostics = DiagnosticSet::empty();
        for failed in report.failed {
            for diagnostic in failed.diagnostics {
                diagnostics.push(coflow_api::Diagnostic::error(
                    diagnostic.code,
                    diagnostic.stage,
                    diagnostic.message,
                ));
            }
        }
        if diagnostics.is_empty() {
            diagnostics.push(coflow_api::Diagnostic::error(
                "WRITE-TXN-NO-OUTCOME",
                "WRITE",
                "mutation transaction produced neither an applied operation nor a failure",
            ));
        }
        Err(diagnostics)
    }
}
