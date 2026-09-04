use super::builtins;
use super::evaluator::CheckEvaluator;
use super::statements;
use super::value::{EvalRecordRef, EvalValue, ValueLocation};
use crate::{
    CheckDiagnostic, CheckDiagnosticContext, CheckProjection, CheckSchemaLocation,
};
use coflow_model::CfdDataModel;
use coflow_language::limits::StructuralLimits;
use coflow_language::{CftSchema, CftSchemaCheckStmt, CftTopLevelCheck, CftType};
use std::cell::RefCell;

pub(super) struct ExecutionContext<'a> {
    pub(super) schema: &'a CftSchema,
    pub(super) model: &'a CfdDataModel,
    pub(super) projection: &'a CheckProjection,
    pub(super) structural_limits: StructuralLimits,
    regex_cache: &'a RefCell<builtins::RegexCache>,
}

impl<'a> ExecutionContext<'a> {
    pub(super) fn new(
        schema: &'a CftSchema,
        model: &'a CfdDataModel,
        projection: &'a CheckProjection,
        structural_limits: StructuralLimits,
        regex_cache: &'a RefCell<builtins::RegexCache>,
    ) -> Self {
        Self {
            schema,
            model,
            projection,
            structural_limits,
            regex_cache,
        }
    }

    pub(super) fn execute_project(
        &self,
        check: &CftTopLevelCheck,
        statement: &CftSchemaCheckStmt,
    ) -> (Vec<CheckDiagnostic>, u64) {
        let mut evaluator = CheckEvaluator::new(
            self.schema,
            self.model,
            None,
            EvalValue::null(),
            self.regex_cache,
            self.structural_limits,
        );
        evaluator.schema_location = Some(CheckSchemaLocation {
            module: check.module.clone(),
            span: check.block.span,
        });
        evaluator.contexts.push(CheckDiagnosticContext::Check {
            name: check.name.to_string(),
        });
        self.configure_projection(&mut evaluator);
        let _ = statements::eval_root_statement(&mut evaluator, statement);
        evaluator.into_execution()
    }

    pub(super) fn execute_record(
        &self,
        meta: &CftType,
        location: ValueLocation,
        statement: &CftSchemaCheckStmt,
        project_dimension: bool,
    ) -> (Vec<CheckDiagnostic>, u64) {
        let mut evaluator = CheckEvaluator::new(
            self.schema,
            self.model,
            Some(location.clone()),
            EvalValue::Record(EvalRecordRef::Resolved(location)),
            self.regex_cache,
            self.structural_limits,
        );
        evaluator.schema_location = Some(CheckSchemaLocation {
            module: meta.module.clone(),
            span: meta.check.as_ref().map_or(meta.span, |check| check.span),
        });
        if project_dimension {
            self.configure_projection(&mut evaluator);
        }
        let _ = statements::eval_root_statement(&mut evaluator, statement);
        evaluator.into_execution()
    }

    fn configure_projection(&self, evaluator: &mut CheckEvaluator<'_>) {
        evaluator.projection_view =
            crate::dimensions::CheckProjectionView::new(self.projection);
        if let Some((dimension, variant)) = self.projection.dimension() {
            evaluator.contexts.insert(
                0,
                CheckDiagnosticContext::Dimension {
                    dimension: dimension.to_string(),
                    variant: variant.to_string(),
                },
            );
        }
    }
}
