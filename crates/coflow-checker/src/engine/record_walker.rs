use super::context::ExecutionContext;
use super::value::ValueLocation;
use crate::limits::{EvaluationBudget, EvaluationCursor, EvaluationKind};
use crate::CheckDiagnostic;
use coflow_language::cft::{CftSchemaCheckStmt, CftType};
use coflow_model::{CfdDiagnostic, CfdErrorCode, CfdRecordId, CfdValue};

pub(super) struct RecordCheckWalker<'a> {
    context: ExecutionContext<'a>,
    root: CfdRecordId,
    owner: &'a CftType,
    diagnostics: Vec<CheckDiagnostic>,
    work: u64,
    budget: EvaluationBudget,
}

impl<'a> RecordCheckWalker<'a> {
    pub(super) fn new(
        context: ExecutionContext<'a>,
        root: CfdRecordId,
        owner: &'a CftType,
    ) -> Self {
        Self {
            budget: EvaluationBudget::new(context.evaluation_limits),
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
            EvaluationCursor::root(),
        );
        (
            self.diagnostics,
            self.work.saturating_add(self.budget.work_used()),
        )
    }

    fn visit_object(
        &mut self,
        actual_type: &str,
        fields: &std::collections::BTreeMap<coflow_language::cft::FieldName, CfdValue>,
        location: ValueLocation,
        statement: &CftSchemaCheckStmt,
        cursor: EvaluationCursor,
    ) {
        let cursor = match self.budget.enter(cursor, EvaluationKind::DataValue, 1) {
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
            let (diagnostics, work) =
                self.context
                    .execute_record(self.owner, location.clone(), statement);
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

            self.visit_value(value, field_location, statement, cursor);
        }
    }

    fn visit_value(
        &mut self,
        value: &CfdValue,
        location: ValueLocation,
        statement: &CftSchemaCheckStmt,
        cursor: EvaluationCursor,
    ) {
        match value {
            CfdValue::Object(object) => self.visit_object(
                object.actual_type(),
                object.fields(),
                location,
                statement,
                cursor,
            ),
            CfdValue::Array(items) => {
                for (index, item) in items.iter().enumerate() {
                    self.visit_value(item, location.index(index), statement, cursor);
                }
            }
            CfdValue::Dict(entries) => {
                for (key, item) in entries {
                    self.visit_value(item, location.dict_key_value(key), statement, cursor);
                }
            }
            CfdValue::OptionSome(value) => {
                self.visit_value(value, location, statement, cursor);
            }
            CfdValue::Ref(_)
            | CfdValue::OptionNone
            | CfdValue::Bool(_)
            | CfdValue::Int(_)
            | CfdValue::Float(_)
            | CfdValue::String(_)
            | CfdValue::FormattedString(_)
            | CfdValue::Enum(_) => {}
        }
    }
}
