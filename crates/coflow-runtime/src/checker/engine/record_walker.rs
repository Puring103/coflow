use super::context::ExecutionContext;
use super::value::ValueLocation;
use crate::checker::dimensions::DimensionVariantAbort;
use crate::checker::CheckDiagnostic;
use crate::data_model::{CfdDiagnostic, CfdErrorCode, CfdRecordId, CfdValue};
use coflow_language::limits::{StructuralBudget, StructureKind, TraversalCursor};
use coflow_language::{CftSchemaCheckStmt, CftType};

pub(super) struct RecordCheckWalker<'a> {
    context: ExecutionContext<'a>,
    root: CfdRecordId,
    owner: &'a CftType,
    diagnostics: Vec<CheckDiagnostic>,
    work: u64,
    budget: StructuralBudget,
}

impl<'a> RecordCheckWalker<'a> {
    pub(super) fn new(
        context: ExecutionContext<'a>,
        root: CfdRecordId,
        owner: &'a CftType,
    ) -> Self {
        Self {
            budget: StructuralBudget::new(context.structural_limits),
            context,
            root,
            owner,
            diagnostics: Vec::new(),
            work: 0,
        }
    }

    pub(super) fn execute(mut self, statement: &CftSchemaCheckStmt) -> (Vec<CheckDiagnostic>, u64) {
        let Some(record) = self.context.model.record(self.root) else {
            return (
                vec![super::internal_diagnostic("unknown check record target")],
                0,
            );
        };
        let direct = self
            .context
            .schema
            .check_owner_applies_to_actual(&self.owner.name, record.actual_type());
        let nested_host = self
            .context
            .schema
            .check_hosts_for_nested_type(&self.owner.name)
            .any(|host| host.as_str() == record.actual_type());
        if !direct && !nested_host {
            return (
                vec![super::internal_diagnostic(
                    "record target cannot contain the statement owner",
                )],
                0,
            );
        }
        let location = ValueLocation::root(self.root);
        self.visit_object(
            record.actual_type(),
            record.fields(),
            location,
            statement,
            true,
            TraversalCursor::root(),
        );
        (
            self.diagnostics,
            self.work.saturating_add(self.budget.work_used()),
        )
    }

    fn visit_object(
        &mut self,
        actual_type: &str,
        fields: &std::collections::BTreeMap<coflow_language::FieldName, CfdValue>,
        location: ValueLocation,
        statement: &CftSchemaCheckStmt,
        project_dimension: bool,
        cursor: TraversalCursor,
    ) {
        let cursor = match self.budget.enter(cursor, StructureKind::DataValue, 1) {
            Ok(cursor) => cursor,
            Err(error) => {
                self.diagnostics.push(
                    CfdDiagnostic::error(CfdErrorCode::CheckBudgetExceeded, error.to_string())
                        .with_primary(Some(self.root), location.blame.path)
                        .into(),
                );
                return;
            }
        };
        if self
            .context
            .schema
            .check_owner_applies_to_actual(&self.owner.name, actual_type)
        {
            let (diagnostics, work) = self.context.execute_record(
                self.owner,
                location.clone(),
                statement,
                project_dimension,
            );
            self.diagnostics.extend(diagnostics);
            self.work = self.work.saturating_add(work);
        }
        for (field, value) in fields {
            if !self
                .context
                .schema
                .field_has_nested_checks(actual_type, field)
            {
                continue;
            }
            let field_location = location.field(field.as_str());
            if project_dimension {
                if let Some(view) =
                    crate::checker::dimensions::CheckProjectionView::new(self.context.projection)
                {
                    match view.materialize(
                        self.context.schema,
                        self.context.model,
                        self.root,
                        field,
                        &field_location,
                    ) {
                        Ok(Some(materialized)) => {
                            self.visit_value(
                                materialized.value,
                                materialized.location,
                                statement,
                                false,
                                cursor,
                            );
                            continue;
                        }
                        Ok(None) => {}
                        Err(DimensionVariantAbort::Skipped) => continue,
                        Err(DimensionVariantAbort::Error {
                            code,
                            location,
                            message,
                        }) => {
                            let location = (*location).unwrap_or(field_location);
                            self.diagnostics.push(
                                CfdDiagnostic::error(code, message)
                                    .with_primary(Some(self.root), location.blame.path)
                                    .into(),
                            );
                            continue;
                        }
                    }
                }
            }
            self.visit_value(value, field_location, statement, false, cursor);
        }
    }

    fn visit_value(
        &mut self,
        value: &CfdValue,
        location: ValueLocation,
        statement: &CftSchemaCheckStmt,
        project_dimension: bool,
        cursor: TraversalCursor,
    ) {
        match value {
            CfdValue::Object(object) => self.visit_object(
                object.actual_type(),
                object.fields(),
                location,
                statement,
                project_dimension,
                cursor,
            ),
            CfdValue::Array(items) => {
                for (index, item) in items.iter().enumerate() {
                    self.visit_value(
                        item,
                        location.index(index),
                        statement,
                        project_dimension,
                        cursor,
                    );
                }
            }
            CfdValue::Dict(entries) => {
                for (key, item) in entries {
                    self.visit_value(
                        item,
                        location.dict_key_value(key),
                        statement,
                        project_dimension,
                        cursor,
                    );
                }
            }
            CfdValue::Ref(_)
            | CfdValue::Null
            | CfdValue::Bool(_)
            | CfdValue::Int(_)
            | CfdValue::Float(_)
            | CfdValue::String(_)
            | CfdValue::FormattedString(_)
            | CfdValue::Enum(_) => {}
        }
    }
}
