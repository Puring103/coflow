use super::builtins;
use super::evaluator::CheckEvaluator;
use super::statements;
use super::value::{EvalRecordRef, EvalValue, ValueLocation};
use crate::{CheckDiagnostic, CheckDiagnosticContext, CheckSchemaLocation};
use coflow_language::cft::{CftSchema, CftSchemaCheckStmt, CftTopLevelCheck, CftType};
use coflow_model::CfdDataModel;
use std::cell::RefCell;

pub(super) struct ExecutionContext<'a> {
    pub(super) schema: &'a CftSchema,
    pub(super) model: &'a CfdDataModel,
    pub(super) evaluation_limits: crate::EvaluationLimits,
    regex_cache: &'a RefCell<builtins::RegexCache>,
}

impl<'a> ExecutionContext<'a> {
    pub(super) fn new(
        schema: &'a CftSchema,
        model: &'a CfdDataModel,
        evaluation_limits: crate::EvaluationLimits,
        regex_cache: &'a RefCell<builtins::RegexCache>,
    ) -> Self {
        Self {
            schema,
            model,
            evaluation_limits,
            regex_cache,
        }
    }

    pub(super) fn execute_project(
        &self,
        check: &CftTopLevelCheck,
        statement: &CftSchemaCheckStmt,
    ) -> (Vec<CheckDiagnostic>, u64, bool) {
        let mut evaluator = CheckEvaluator::new(
            self.schema,
            self.model,
            None,
            EvalValue::null(),
            self.regex_cache,
            self.evaluation_limits,
        );
        evaluator.schema_location = Some(CheckSchemaLocation {
            module: check.module.clone(),
            span: check.block.span,
        });
        evaluator.contexts.push(CheckDiagnosticContext::Check {
            name: check.name.to_string(),
        });
        if statements::eval_root_statement(&mut evaluator, statement)
            == super::evaluator::EvalFlow::Continue
        {
            let _ =
                statements::apply_block_message(&mut evaluator, 0, check.block.message.as_ref());
        }
        evaluator.into_execution()
    }

    pub(super) fn execute_record(
        &self,
        meta: &CftType,
        rule: &coflow_language::cft::CftSchemaCheckBlock,
        rule_id: &str,
        location: ValueLocation,
        statement: &CftSchemaCheckStmt,
    ) -> (Vec<CheckDiagnostic>, u64, bool) {
        let mut evaluator = CheckEvaluator::new(
            self.schema,
            self.model,
            Some(location.clone()),
            EvalValue::Record(EvalRecordRef::Resolved(location)),
            self.regex_cache,
            self.evaluation_limits,
        );
        evaluator.schema_location = Some(CheckSchemaLocation {
            module: meta.module.clone(),
            span: rule.span,
        });
        evaluator.contexts.push(CheckDiagnosticContext::Check {
            name: rule_id.to_string(),
        });
        if statements::eval_root_statement(&mut evaluator, statement)
            == super::evaluator::EvalFlow::Continue
        {
            let _ = statements::apply_block_message(&mut evaluator, 0, rule.message.as_ref());
        }
        evaluator.into_execution()
    }
}
