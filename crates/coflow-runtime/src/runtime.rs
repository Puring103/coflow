use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};

use crate::api::{Diagnostic, DiagnosticSet, Severity, WriterCapabilities};
use crate::catalog::CfdSourceCatalog;
use crate::data_model::CfdDataModel;
use crate::data_model::{CfdPathSegment, CfdValue};
use crate::project::Project;
use coflow_language::CftSchema;

use crate::project_schema::{
    open_project_schema_attempt, open_project_schema_session, SchemaTextOverride,
};
use crate::session::{ProjectSchemaSession, ProjectSession};
use crate::session_build::{
    open_project_session, open_project_session_from_schema,
    open_project_session_with_source_overrides, SessionOpenOptions,
};
use crate::{
    CreateRecordDraft, DataSourceTextOverride, DefaultMaterialization, DimensionValueCoordinate,
    DimensionValueExpectation, MutationFields, MutationOp, MutationReport, MutationRequest,
    MutationValue, ProjectQueries, RecordCoordinate, WriteOutcome,
};

#[derive(Debug, Clone)]
pub struct Runtime {
    catalog: CfdSourceCatalog,
}

/// Owns the published schema generation for one project.
///
/// Hosts call [`Self::refresh`] after filesystem-backed project changes;
/// the runtime, rather than an editor, decides whether schema inputs changed.
#[derive(Debug)]
pub struct ProjectRuntime {
    project: Project,
    published: Option<SchemaGeneration>,
    attempted: Option<SchemaGeneration>,
}

/// Runtime-private cache record for one immutable schema generation.
///
/// Keeping parsed modules and the semantic schema behind the same fingerprint
/// ensures language hosts never reparse text that the compiler already read.
#[derive(Debug)]
struct SchemaGeneration {
    fingerprint: u64,
    session: ProjectSchemaSession,
}

impl ProjectRuntime {
    #[must_use]
    pub const fn new(project: Project) -> Self {
        Self {
            project,
            published: None,
            attempted: None,
        }
    }

    #[must_use]
    pub fn schema(&self) -> Option<&ProjectSchemaSession> {
        self.published
            .as_ref()
            .map(|generation| &generation.session)
    }

    /// Returns the latest build attempt, including an invalid CFT module set.
    /// Language tooling uses this for diagnostics while [`Self::schema`] keeps
    /// pointing at the last successfully published schema.
    #[must_use]
    pub fn latest_attempt(&self) -> Option<&ProjectSchemaSession> {
        self.attempted
            .as_ref()
            .or(self.published.as_ref())
            .map(|generation| &generation.session)
    }

    #[must_use]
    pub fn into_latest_attempt(self) -> Option<ProjectSchemaSession> {
        self.attempted
            .or(self.published)
            .map(|generation| generation.session)
    }

    /// Refreshes the published schema only when CFT text or dimension variants change.
    ///
    /// A failed rebuild leaves the last successful generation available for
    /// language tooling, while the returned diagnostics still prevent callers
    /// from treating the failed refresh as a valid project build.
    ///
    /// # Errors
    ///
    /// Returns project, schema, or source diagnostics when the candidate cannot be built.
    pub fn refresh(&mut self) -> Result<bool, DiagnosticSet> {
        self.refresh_with_overrides(&[])
    }

    /// Rebuilds from a host's current in-memory CFT document snapshots.
    ///
    /// A failed candidate is retained only as `latest_attempt` for diagnostics;
    /// it never replaces the published generation used by semantic consumers.
    ///
    /// # Errors
    ///
    /// Returns project, schema, or source diagnostics when the candidate cannot be built.
    pub fn refresh_with_overrides(
        &mut self,
        overrides: &[SchemaTextOverride],
    ) -> Result<bool, DiagnosticSet> {
        let fingerprint = schema_input_fingerprint(&self.project, overrides)?;
        if self
            .attempted
            .as_ref()
            .is_some_and(|generation| generation.fingerprint == fingerprint)
        {
            return self.attempt_result();
        }
        if self
            .published
            .as_ref()
            .is_some_and(|generation| generation.fingerprint == fingerprint)
        {
            self.attempted = None;
            return Ok(false);
        }

        let diagnostics = self.project.schema_diagnostic_set();
        let session = open_project_schema_attempt(self.project.clone(), diagnostics, overrides)?;
        let generation = SchemaGeneration {
            fingerprint,
            session,
        };
        let diagnostics = generation.session.diagnostics().clone().into_set();
        let changed = self
            .published
            .as_ref()
            .is_none_or(|published| published.fingerprint != fingerprint);

        // Publish only a fully valid schema; failed editor text must not
        // invalidate semantic queries that still rely on the last good one.
        self.attempted = Some(generation);
        if diagnostics.is_empty() {
            self.published = self.attempted.take();
            return Ok(changed);
        }
        Err(diagnostics)
    }

    fn attempt_result(&self) -> Result<bool, DiagnosticSet> {
        let Some(attempt) = self.attempted.as_ref() else {
            return Ok(false);
        };
        let diagnostics = attempt.session.diagnostics().clone().into_set();
        if diagnostics.is_empty() {
            Ok(false)
        } else {
            Err(diagnostics)
        }
    }
}

#[cfg(test)]
mod project_runtime_tests {
    #![allow(clippy::expect_used)]

    use std::fs;

    use super::ProjectRuntime;
    use crate::{project::normalize_path, Project, SchemaTextOverride};

    #[test]
    fn reverting_to_published_schema_discards_failed_attempt() {
        let root = tempfile::tempdir().expect("temp project");
        let schema_path = root.path().join("schema.cft");
        fs::write(&schema_path, "type Item { value: int; }\n").expect("write schema");
        fs::write(
            root.path().join("coflow.yaml"),
            "schema: schema.cft\ndata: data/\ncodegen:\n  - language: csharp\n    dir: generated/\n",
        )
        .expect("write config");
        let project = Project::open_schema_only(Some(root.path())).expect("open project");
        let mut runtime = ProjectRuntime::new(project);

        assert_eq!(runtime.refresh(), Ok(true));
        assert!(runtime
            .refresh_with_overrides(&[SchemaTextOverride {
                requested_module: None,
                normalized_path: normalize_path(&schema_path),
                source: "type Item { value: Missing; }\n".to_string(),
            }])
            .is_err());
        assert!(runtime
            .latest_attempt()
            .is_some_and(|attempt| attempt.has_diagnostics()));

        assert_eq!(runtime.refresh(), Ok(false));
        assert!(runtime
            .latest_attempt()
            .is_some_and(|attempt| !attempt.has_diagnostics()));
    }
}

fn schema_input_fingerprint(
    project: &Project,
    overrides: &[SchemaTextOverride],
) -> Result<u64, DiagnosticSet> {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for module in project.schema_sources()? {
        module.module_id.hash(&mut hasher);
        module.canonical_path.hash(&mut hasher);
        let source = overrides
            .iter()
            .enumerate()
            .rev()
            .find(|(_, source_override)| {
                source_override
                    .requested_module
                    .as_deref()
                    .is_some_and(|requested| requested == module.module_id)
                    || crate::project::normalize_path(&module.canonical_path)
                        == source_override.normalized_path
            })
            .map_or(&module.source, |(_, source_override)| {
                &source_override.source
            });
        source.hash(&mut hasher);
    }
    for source_override in overrides {
        source_override.requested_module.hash(&mut hasher);
        source_override.normalized_path.hash(&mut hasher);
        source_override.source.hash(&mut hasher);
    }
    for (dimension, config) in &project.config().dimensions {
        dimension.hash(&mut hasher);
        config.variants.hash(&mut hasher);
    }
    Ok(hasher.finish())
}

impl Runtime {
    #[must_use]
    pub fn new() -> Self {
        Self {
            catalog: fixed_cfd_catalog(),
        }
    }

    /// Builds a schema-only session without loading project data.
    ///
    /// # Errors
    ///
    /// Returns unrecoverable project/config/schema I/O diagnostics.
    pub fn open_schema_session(project: Project) -> Result<ProjectSchemaSession, DiagnosticSet> {
        open_project_schema_session(project)
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
        open_project_session(project, &self.catalog, SessionOpenOptions::read_only())
            .map(ReadOnlyProjectSession::new)
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
        open_project_session_with_source_overrides(
            project,
            &self.catalog,
            SessionOpenOptions::read_only(),
            source_overrides,
        )
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
        open_project_session(project, &self.catalog, SessionOpenOptions::build())
            .map(BuildProjectSession::new)
    }

    /// Opens a mutation-capable session without generating dimension files.
    /// The session owns the CFD catalog used by every command and rebuild.
    ///
    /// # Errors
    ///
    /// Returns unrecoverable project/config/schema I/O diagnostics.
    pub fn open_write_session(
        &self,
        project: Project,
    ) -> Result<WriteProjectSession, DiagnosticSet> {
        open_project_session(project, &self.catalog, SessionOpenOptions::read_only())
            .map(|session| WriteProjectSession::new(session, self.catalog.clone()))
    }

    /// Opens a write-capable data session from a runtime-built schema generation.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when project sources cannot be opened against the schema.
    pub fn open_write_session_from_schema(
        &self,
        schema: ProjectSchemaSession,
    ) -> Result<WriteProjectSession, DiagnosticSet> {
        open_project_session_from_schema(schema, &self.catalog, SessionOpenOptions::read_only())
            .map(|session| WriteProjectSession::new(session, self.catalog.clone()))
    }
}

fn fixed_cfd_catalog() -> CfdSourceCatalog {
    CfdSourceCatalog::default()
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
    const fn new(session: ProjectSession) -> Self {
        Self { session }
    }

    #[must_use]
    pub const fn queries(&self) -> ProjectQueries<'_> {
        ProjectQueries::new(&self.session, 0)
    }

    #[must_use]
    pub fn schema(&self) -> &CftSchema {
        self.session.schema()
    }

    #[must_use]
    pub fn model(&self) -> &CfdDataModel {
        self.session.model()
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
    const fn new(session: ProjectSession) -> Self {
        Self { session }
    }

    #[must_use]
    pub const fn queries(&self) -> ProjectQueries<'_> {
        ProjectQueries::new(&self.session, 0)
    }

    #[must_use]
    pub fn schema(&self) -> &CftSchema {
        self.session.schema()
    }

    #[must_use]
    pub fn model(&self) -> &CfdDataModel {
        self.session.model()
    }

    #[must_use]
    pub fn into_diagnostics(self) -> DiagnosticSet {
        self.session.into_diagnostics()
    }
}

#[derive(Debug)]
pub struct WriteProjectSession {
    session: ProjectSession,
    catalog: CfdSourceCatalog,
    revision: u64,
}

impl WriteProjectSession {
    const fn new(session: ProjectSession, catalog: CfdSourceCatalog) -> Self {
        Self {
            session,
            catalog,
            revision: 0,
        }
    }

    #[must_use]
    pub const fn queries(&self) -> ProjectQueries<'_> {
        ProjectQueries::new(&self.session, self.revision)
    }

    #[must_use]
    pub const fn project(&self) -> &Project {
        &self.session.project
    }

    /// Render one effective field value using the CFD value grammar.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when the field path does not exist or its value
    /// cannot be represented by the CFD value grammar.
    pub fn render_cell_text(
        &self,
        coordinate: &RecordCoordinate,
        path: &[CfdPathSegment],
    ) -> Result<String, DiagnosticSet> {
        let value = self
            .queries()
            .field_value(&coordinate.actual_type, &coordinate.key, path)
            .ok_or_else(|| {
                DiagnosticSet::one(Diagnostic {
                    code: "MUTATION-PATH".to_string(),
                    stage: "MUTATION".to_string(),
                    severity: Severity::Error,
                    message: "selected field was not found".to_string(),
                    primary: None,
                    related: Vec::new(),
                    contexts: Vec::new(),
                })
            })?;
        crate::mutation::render_cell_text_value(value)
    }

    /// Parse CFD value text using the schema type at one field path.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when the field path is invalid or `text` does not
    /// conform to the field's schema type.
    pub fn parse_cell_text(
        &self,
        coordinate: &RecordCoordinate,
        path: &[CfdPathSegment],
        text: &str,
    ) -> Result<CfdValue, DiagnosticSet> {
        crate::mutation::parse_cell_text_value(
            &self.session,
            &coordinate.actual_type,
            &coordinate.key,
            path,
            text,
        )
    }

    #[must_use]
    pub fn writer_capabilities_for_file(&self, file: &str) -> WriterCapabilities {
        self.queries().writer_capabilities_for_file(file)
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
    pub fn create_record_draft(&self, type_name: &str) -> Result<CreateRecordDraft, DiagnosticSet> {
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

    /// Build a default collection item using the concrete types in a record.
    pub fn default_collection_item_value_for_record(
        &self,
        coordinate: &RecordCoordinate,
        path: &[CfdPathSegment],
    ) -> Result<CfdValue, DiagnosticSet> {
        self.session
            .default_collection_item_value_for_record(coordinate, path)
    }

    /// Apply a batch of mutation commands using the CFD catalog owned by this
    /// capability.
    pub fn apply_mutation(&mut self, request: MutationRequest) -> MutationReport {
        let report = self.session.apply_mutation(&self.catalog, request);
        if report.generation_changed {
            self.revision = self.revision.saturating_add(1);
        }
        report
    }

    /// Writes one field and returns its writer outcome.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when the mutation is rejected or produces no
    /// applied operation.
    pub fn write_field(
        &mut self,
        actual_type: &str,
        key: &str,
        path: &[CfdPathSegment],
        new_value: &CfdValue,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        self.apply_one(MutationOp::SetField {
            record: validated_coordinate(actual_type, key)?,
            file: None,
            path: path.to_vec(),
            value: MutationValue::Cfd(new_value.clone()),
        })
    }

    /// Writes one record-owned dimension variant value.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when the coordinate is invalid or the managed
    /// dimension source cannot be written.
    pub fn write_dimension_value(
        &mut self,
        coordinate: DimensionValueCoordinate,
        new_value: &CfdValue,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        self.apply_one(MutationOp::SetDimensionValue {
            coordinate,
            expected: DimensionValueExpectation::Any,
            value: MutationValue::Cfd(new_value.clone()),
        })
    }

    /// Clears one record-owned dimension variant so it becomes missing.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when the coordinate is invalid or the managed
    /// dimension source cannot be written.
    pub fn clear_dimension_value(
        &mut self,
        coordinate: DimensionValueCoordinate,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        self.apply_one(MutationOp::ClearDimensionValue {
            coordinate,
            expected: DimensionValueExpectation::Any,
        })
    }

    /// Renames one record key and its references.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when the mutation is rejected or produces no
    /// applied operation.
    pub fn rename_record_key(
        &mut self,
        actual_type: &str,
        old_key: &str,
        new_key: &str,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        self.apply_one(MutationOp::RenameRecord {
            record: validated_coordinate(actual_type, old_key)?,
            file: None,
            new_key: new_key.to_string(),
        })
    }

    /// Inserts one record into the selected source.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when the mutation is rejected or produces no
    /// applied operation.
    pub fn insert_record(
        &mut self,
        file: &str,
        record_key: &str,
        actual_type: &str,
        fields: &BTreeMap<String, CfdValue>,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        self.apply_one(MutationOp::InsertRecord {
            file: file.to_string(),
            actual_type: actual_type.to_string(),
            key: record_key.to_string(),
            fields: MutationFields::Cfd(fields.clone()),
            materialization: DefaultMaterialization::Minimal,
        })
    }

    /// Deletes one record and updates affected references.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when the mutation is rejected or produces no
    /// applied operation.
    pub fn delete_record(
        &mut self,
        actual_type: &str,
        key: &str,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        self.apply_one(MutationOp::DeleteRecord {
            record: validated_coordinate(actual_type, key)?,
            file: None,
        })
    }

    /// Atomically exchange two records inside the same physical source container.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when either record is missing, the records belong
    /// to different containers, or the writer cannot persist record order.
    pub fn swap_records(
        &mut self,
        first: &RecordCoordinate,
        second: &RecordCoordinate,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        self.apply_one(MutationOp::SwapRecords {
            first: first.clone(),
            second: second.clone(),
            file: None,
        })
    }

    /// Move one record to a zero-based final index in its physical container.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when the record is missing, the index is outside
    /// the container, or the writer cannot persist record order.
    pub fn move_record(
        &mut self,
        record: &RecordCoordinate,
        target_index: usize,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        self.apply_one(MutationOp::MoveRecord {
            record: record.clone(),
            target_index,
            file: None,
        })
    }

    /// Move a record to a zero-based insertion index in another source file.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when the destination cannot host the record type,
    /// the insertion index is invalid, or either source cannot participate in
    /// the atomic transfer transaction.
    pub fn transfer_record(
        &mut self,
        record: &RecordCoordinate,
        destination_file: &str,
        target_index: usize,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        self.apply_one(MutationOp::TransferRecord {
            record: record.clone(),
            destination_file: destination_file.to_string(),
            target_index,
            source_file: None,
        })
    }

    fn apply_one(&mut self, op: MutationOp) -> Result<WriteOutcome, DiagnosticSet> {
        let report = self.apply_mutation(MutationRequest {
            stop_on_write_error: true,
            ops: vec![op],
        });
        if let Some(applied) = report.applied.into_iter().next() {
            return Ok(applied.outcome);
        }
        let mut diagnostics = DiagnosticSet::empty();
        for failed in report.failed {
            diagnostics.extend(failed.into_source_diagnostics());
        }
        if diagnostics.is_empty() {
            diagnostics.push(Diagnostic::error(
                "WRITE-TXN-NO-OUTCOME",
                "WRITE",
                "mutation transaction produced neither an applied operation nor a failure",
            ));
        }
        Err(diagnostics)
    }
}

fn validated_coordinate(actual_type: &str, key: &str) -> Result<RecordCoordinate, DiagnosticSet> {
    RecordCoordinate::try_new(actual_type, key).map_err(|error| {
        DiagnosticSet::one(Diagnostic::error(
            "MUTATION-COORDINATE",
            "MUTATION",
            error.to_string(),
        ))
    })
}
