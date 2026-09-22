use super::SourceValidationContext;
use crate::api::{Diagnostic, DiagnosticSet, Severity, WriterCapabilities};
use crate::data_model::{CfdPathSegment, CfdValue};
use crate::session::ProjectSession;
use crate::{
    CreateRecordDraft, DefaultMaterialization, MutationAppliedOp, MutationReport, MutationRequest,
    Project, ProjectFileUpdate, ProjectQueries, RecordCoordinate,
};
use std::sync::Arc;

#[derive(Debug)]
pub struct WriteProjectSession {
    pub(super) identity: Arc<()>,
    pub(super) session: Arc<ProjectSession>,
    pub(super) revision: u64,
    schema_validation: Arc<std::sync::Mutex<crate::SchemaCache>>,
}

impl WriteProjectSession {
    pub(super) fn new(session: ProjectSession) -> Self {
        Self {
            schema_validation: Arc::new(std::sync::Mutex::new(crate::SchemaCache::new(
                session.project.clone(),
            ))),
            identity: Arc::new(()),
            session: Arc::new(session),
            revision: 0,
        }
    }

    /// 持有不可变代际供耗时命令使用，宿主取得后即可释放项目锁。
    pub fn snapshot(&self) -> super::ProjectSnapshot {
        super::ProjectSnapshot {
            session: Arc::clone(&self.session),
            revision: self.revision,
        }
    }
    pub fn diff_against_head(&self) -> Result<crate::ProjectDiff, DiagnosticSet> {
        self.snapshot().diff_against_head()
    }
    #[must_use]
    pub fn queries(&self) -> ProjectQueries<'_> {
        ProjectQueries::new(&self.session, self.revision)
    }

    #[must_use]
    pub fn project(&self) -> &Project {
        &self.session.project
    }

    /// Render one effective field value using the CFD value grammar.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when the field path does not exist.
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
        Ok(crate::cell_value::render_cell_value(value))
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

    /// 克隆一份只读校验上下文；克隆只共享 `Arc` 与源缓存，开销很小。
    ///
    /// 宿主可先短暂持有会话读锁取得上下文，再释放锁执行 [`SourceValidationContext::validate`]，
    /// 避免占用会话锁进行长耗时的项目校验。
    #[must_use]
    pub fn validation_context(&self) -> SourceValidationContext {
        SourceValidationContext {
            schema_cache: Arc::clone(&self.schema_validation),
            schema: Arc::clone(&self.session.schema),
            source_data: self.session.source_data.clone(),
        }
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
    ///
    /// # Errors
    ///
    /// Returns diagnostics when the record/path is invalid or no valid
    /// reference target exists.
    pub fn default_collection_item_value_for_record(
        &self,
        coordinate: &RecordCoordinate,
        path: &[CfdPathSegment],
    ) -> Result<CfdValue, DiagnosticSet> {
        self.session
            .default_collection_item_value_for_record(coordinate, path)
    }

    /// Apply a batch of mutation commands in its own staged write workspace.
    pub fn apply_mutation(&mut self, request: MutationRequest) -> MutationReport {
        self.apply_mutation_with_project_files(request, |_, _| Ok(Vec::new()))
    }

    /// Apply a mutation and publish application-owned project files in the
    /// same file transaction as the changed CFD sources.
    pub fn apply_mutation_with_project_files<F>(
        &mut self,
        request: MutationRequest,
        prepare_files: F,
    ) -> MutationReport
    where
        F: FnOnce(
            ProjectQueries<'_>,
            &[MutationAppliedOp],
        ) -> Result<Vec<ProjectFileUpdate>, DiagnosticSet>,
    {
        let Some(next_revision) = self.revision.checked_add(1) else {
            let diagnostics = DiagnosticSet::one(Diagnostic::error(
                "MUTATION-CONFLICT",
                "MUTATION",
                "project revision exhausted",
            ));
            return MutationReport {
                changed_records: Default::default(),
                write_ok: false,
                check_ok: false,
                generation_changed: false,
                applied: Vec::new(),
                affected_files: Vec::new(),
                written_files: Vec::new(),
                diagnostics: diagnostics.flat_diagnostics(),
                failed: vec![crate::MutationFailedOp::from_diagnostics(
                    0,
                    "mutation",
                    diagnostics,
                )],
            };
        };
        let report =
            Arc::make_mut(&mut self.session).apply_mutation(request, |candidate, applied| {
                prepare_files(ProjectQueries::new(candidate, next_revision), applied)
            });
        if report.generation_changed {
            self.revision = next_revision;
        }
        report
    }
}
