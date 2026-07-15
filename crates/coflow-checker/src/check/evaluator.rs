use super::access;
use super::builtin_calls::{self, CallSignature, CallSignatureError, CallTarget};
use super::builtin_values;
use super::builtins::Builtin;
use super::deps::DependencyCollector;
use super::diagnostics::format_value_for_message;
use super::dimensions::{self, DimensionVariantAbort};
use super::enum_values;
use super::evaluation_trace::EvaluationTrace;
use super::fields;
use super::ops::{self, OpsResult};
use super::quantifiers;
use super::value::{CheckValue, LocatedCheckValue, ValueLocation};
use coflow_cft::{
    CftSchemaBinOp, CftSchemaCheckExpr, CftSchemaCmpOp, CftSchemaUnaryOp, CftSchema,
};
use coflow_data_model::{CfdDataModel, CfdDiagnostic, CfdErrorCode, CfdRecordId};
use coflow_structure::{StructuralBudget, StructuralLimits, StructureKind, TraversalCursor};
use std::collections::BTreeMap;

use super::value::CheckRecordRef;

pub(super) struct CheckEvaluator<'a> {
    pub(super) schema: &'a CftSchema,
    pub(super) model: &'a CfdDataModel,
    pub(super) check_origin: ValueLocation,
    pub(super) current: CheckValue,
    pub(super) scopes: Vec<BTreeMap<String, LocatedCheckValue>>,
    pub(super) contexts: Vec<String>,
    pub(super) diagnostics: Vec<CfdDiagnostic>,
    deps: DependencyCollector,
    pub(super) dimension_round: Option<dimensions::DimensionRoundView>,
    trace: Option<EvaluationTrace>,
    budget: StructuralBudget,
    eval_stack: Vec<TraversalCursor>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EvalFlow {
    Continue,
    Skipped,
    HardStop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EvalAbort {
    Skipped,
    Error,
}

pub(super) type EvalResult<T> = Result<T, EvalAbort>;

impl<'a> CheckEvaluator<'a> {
    pub(super) fn new(
        schema: &'a CftSchema,
        model: &'a CfdDataModel,
        check_origin: ValueLocation,
        current: CheckValue,
        mut deps: DependencyCollector,
        structural_limits: StructuralLimits,
    ) -> Self {
        let initial_top = match &current {
            CheckValue::Record(record) => record.top_record_id(),
            _ => None,
        };
        if let Some(record_id) = initial_top {
            deps.note_read_from(record_id);
        }
        Self {
            schema,
            model,
            check_origin,
            current,
            scopes: Vec::new(),
            contexts: Vec::new(),
            diagnostics: Vec::new(),
            deps,
            dimension_round: None,
            trace: None,
            budget: StructuralBudget::new(structural_limits),
            eval_stack: Vec::new(),
        }
    }

    pub(super) fn into_outputs(self) -> (Vec<CfdDiagnostic>, DependencyCollector) {
        (self.diagnostics, self.deps)
    }

    pub(super) fn note_read_from(&mut self, target: CfdRecordId) {
        self.deps.note_read_from(target);
    }

    pub(super) fn eval_ops<T>(&mut self, result: OpsResult<T>) -> EvalResult<T> {
        result.map_err(|err| {
            let (code, location, message) = err.into_parts();
            self.diag_at(code, location, message);
            EvalAbort::Error
        })
    }

    pub(super) fn apply_dimension_variant(
        &mut self,
        record: &CheckRecordRef,
        field_name: &str,
        located: &mut LocatedCheckValue,
    ) -> EvalResult<()> {
        match dimensions::apply_dimension_variant(
            self.model,
            self.dimension_round.as_ref(),
            record,
            field_name,
            located,
            &mut self.budget,
        ) {
            Ok(Some(record_id)) => {
                self.note_read_from(record_id);
                Ok(())
            }
            Ok(None) => Ok(()),
            Err(DimensionVariantAbort::Skipped) => Err(EvalAbort::Skipped),
            Err(DimensionVariantAbort::Error {
                code,
                location,
                message,
            }) => {
                self.diag_at(code, location, message);
                Err(EvalAbort::Error)
            }
        }
    }

    pub(super) fn eval_expr(&mut self, expr: &CftSchemaCheckExpr) -> EvalResult<LocatedCheckValue> {
        let parent = self
            .eval_stack
            .last()
            .copied()
            .unwrap_or_else(TraversalCursor::root);
        let cursor = match self.budget.enter(parent, StructureKind::CheckEvaluation, 1) {
            Ok(cursor) => cursor,
            Err(error) => {
                self.diag(CfdErrorCode::CheckBudgetExceeded, error.to_string());
                return Err(EvalAbort::Error);
            }
        };
        self.eval_stack.push(cursor);
        let result = super::expressions::eval_expr(self, expr);
        let _ = self.eval_stack.pop();
        if let Ok(value) = &result {
            if let Some(trace) = &mut self.trace {
                trace.record(expr, value, self.model);
            }
        }
        result
    }

    pub(super) fn charge_work_at(
        &mut self,
        kind: StructureKind,
        work: u64,
        location: Option<ValueLocation>,
    ) -> EvalResult<()> {
        self.budget.charge_work(kind, work).map_err(|error| {
            self.diag_at(
                CfdErrorCode::CheckBudgetExceeded,
                location,
                error.to_string(),
            );
            EvalAbort::Error
        })
    }

    pub(super) fn charge_collection_work(&mut self, value: &LocatedCheckValue) -> EvalResult<()> {
        let work = value
            .value
            .collection_len()
            .map_or(0, |length| u64::try_from(length).unwrap_or(u64::MAX));
        self.charge_work_at(StructureKind::CheckEvaluation, work, value.location.clone())
    }

    pub(super) fn eval_expr_with_trace(
        &mut self,
        expr: &CftSchemaCheckExpr,
    ) -> (EvalResult<LocatedCheckValue>, EvaluationTrace) {
        debug_assert!(self.trace.is_none());
        self.trace = Some(EvaluationTrace::for_explanation(expr));
        let result = self.eval_expr(expr);
        let trace = self.trace.take().unwrap_or_default();
        (result, trace)
    }

    pub(super) fn note_comparison_failure(
        &mut self,
        lhs: &CftSchemaCheckExpr,
        op: CftSchemaCmpOp,
        rhs: &CftSchemaCheckExpr,
        location: Option<ValueLocation>,
    ) {
        if let Some(trace) = &mut self.trace {
            trace.note_comparison_failure(lhs, op, rhs, location);
        }
    }

    pub(super) fn note_unique_failure(&mut self, collection: &CftSchemaCheckExpr, detail: String) {
        if let Some(trace) = &mut self.trace {
            trace.note_unique_failure(collection, detail);
        }
    }

    pub(super) fn eval_name(&mut self, name: &str) -> EvalResult<LocatedCheckValue> {
        for scope in self.scopes.iter().rev() {
            if let Some(value) = scope.get(name) {
                return Ok(value.clone());
            }
        }
        let current_field = fields::current_field(
            self.schema,
            self.model,
            &self.current,
            name,
            &mut self.budget,
        );
        if let Some(mut value) = self.eval_ops(current_field)? {
            if let CheckValue::Record(record) = &value.value {
                if let Some(id) = record.top_record_id() {
                    self.note_read_from(id);
                }
            }
            if let CheckValue::Record(record) = self.current.clone() {
                self.apply_dimension_variant(&record, name, &mut value)?;
            }
            return Ok(value);
        }
        if let Some(value) = self.schema.resolve_const(name) {
            return Ok(LocatedCheckValue::value(CheckValue::from_const(&value.value)));
        }
        if self.schema.resolve_enum(name).is_some() {
            return Ok(LocatedCheckValue::value(CheckValue::EnumNamespace(
                name.to_string(),
            )));
        }
        self.diag(
            CfdErrorCode::CheckEvalTypeError,
            format!("未知 check 值 `{name}`"),
        );
        Err(EvalAbort::Error)
    }

    pub(super) fn eval_field(
        &mut self,
        target: LocatedCheckValue,
        name: &str,
    ) -> EvalResult<LocatedCheckValue> {
        let target_record = match &target.value {
            CheckValue::Record(record) => Some(record.clone()),
            _ => None,
        };
        let field = fields::field_value(self.schema, self.model, target, name, &mut self.budget);
        let mut result = self.eval_ops(field)?;
        if let CheckValue::Record(record) = &result.value {
            if let Some(id) = record.top_record_id() {
                self.note_read_from(id);
            }
        }
        if let Some(record) = target_record {
            self.apply_dimension_variant(&record, name, &mut result)?;
        }
        Ok(result)
    }

    pub(super) fn eval_index(
        &mut self,
        target: LocatedCheckValue,
        index: LocatedCheckValue,
    ) -> EvalResult<LocatedCheckValue> {
        let result = access::index_value(target, index, self.model, &mut self.budget);
        self.eval_ops(result)
    }

    pub(super) fn quantifier_item(
        &mut self,
        collection: &LocatedCheckValue,
        index: usize,
    ) -> EvalResult<Option<LocatedCheckValue>> {
        let result = quantifiers::quantifier_item(collection, index, self.model, &mut self.budget);
        self.eval_ops(result)
    }

    pub(super) fn eval_call(
        &mut self,
        name: &str,
        args: &[CftSchemaCheckExpr],
    ) -> EvalResult<LocatedCheckValue> {
        let signature = self.resolve_call_signature(CallSignature::resolve_function(
            name,
            args.len(),
            self.schema.resolve_enum(name).is_some(),
        ))?;

        match signature.target {
            CallTarget::EnumConstructor => {
                let arg = &args[0];
                let arg_value = self.eval_expr(arg)?;
                let arg_kind = arg_value.value.clone();
                let CheckValue::Int(value) = arg_value.value else {
                    self.diag_at(
                        CfdErrorCode::CheckEvalTypeError,
                        arg_value.location,
                        format!(
                            "枚举构造函数参数不是 int: 实际为 {}",
                            format_value_for_message(&arg_kind)
                        ),
                    );
                    return Err(EvalAbort::Error);
                };
                Ok(LocatedCheckValue::value(CheckValue::Enum(
                    enum_values::enum_with_value(self.schema, name, value),
                )))
            }
            CallTarget::Builtin(builtin) => self.eval_builtin_call(builtin, args),
        }
    }

    pub(super) fn eval_builtin_call(
        &mut self,
        builtin: Builtin,
        args: &[CftSchemaCheckExpr],
    ) -> EvalResult<LocatedCheckValue> {
        match builtin {
            Builtin::Len => {
                let arg = &args[0];
                let arg_value = self.eval_expr(arg)?;
                self.eval_ops(builtin_values::len_value(arg_value))
            }
            Builtin::Contains => {
                let collection = self.eval_expr(&args[0])?;
                self.charge_collection_work(&collection)?;
                let value = self.eval_expr(&args[1])?;
                let contains = builtin_values::contains_value(
                    &collection,
                    &value.value,
                    self.model,
                    &mut self.budget,
                );
                Ok(LocatedCheckValue::new(
                    CheckValue::Bool(self.eval_ops(contains)?),
                    collection.location.clone(),
                ))
            }
            Builtin::Unique => {
                let arg = &args[0];
                let arg_value = self.eval_expr(arg)?;
                self.eval_unique(arg, arg_value)
            }
            Builtin::Min | Builtin::Max => self.eval_min_max(builtin, args),
            Builtin::Sum => self.eval_sum(args),
            Builtin::Keys => {
                let arg = &args[0];
                let arg_value = self.eval_expr(arg)?;
                self.charge_collection_work(&arg_value)?;
                self.eval_ops(builtin_values::keys_value(arg_value))
            }
            Builtin::Values => {
                let arg = &args[0];
                let arg_value = self.eval_expr(arg)?;
                self.charge_collection_work(&arg_value)?;
                self.eval_ops(builtin_values::values_value(arg_value))
            }
            Builtin::Matches => {
                let value = self.eval_expr(&args[0])?;
                let pattern =
                    self.resolve_call_signature(builtin_calls::matches_pattern_arg(&args[1]))?;
                self.eval_ops(builtin_values::matches_value(value, pattern))
            }
        }
    }

    pub(super) fn eval_method_call(
        &mut self,
        receiver: &CftSchemaCheckExpr,
        name: &str,
        args: &[CftSchemaCheckExpr],
    ) -> EvalResult<LocatedCheckValue> {
        let signature =
            self.resolve_call_signature(CallSignature::resolve_method(name, args.len()))?;
        let CallTarget::Builtin(builtin) = signature.target else {
            self.diag(
                CfdErrorCode::CheckEvalTypeError,
                "method calls cannot resolve to enum constructors",
            );
            return Err(EvalAbort::Error);
        };

        let receiver_value = self.eval_expr(receiver)?;
        match builtin {
            Builtin::Len => self.eval_ops(builtin_values::len_value(receiver_value)),
            Builtin::Contains => {
                self.charge_collection_work(&receiver_value)?;
                let value = self.eval_expr(&args[0])?;
                let contains = builtin_values::contains_value(
                    &receiver_value,
                    &value.value,
                    self.model,
                    &mut self.budget,
                );
                Ok(LocatedCheckValue::new(
                    CheckValue::Bool(self.eval_ops(contains)?),
                    receiver_value.location.clone(),
                ))
            }
            Builtin::Unique => self.eval_unique(receiver, receiver_value),
            Builtin::Min | Builtin::Max => self.eval_min_max_value(builtin, receiver_value),
            Builtin::Sum => self.eval_sum_value(receiver_value),
            Builtin::Keys => {
                self.charge_collection_work(&receiver_value)?;
                self.eval_ops(builtin_values::keys_value(receiver_value))
            }
            Builtin::Values => {
                self.charge_collection_work(&receiver_value)?;
                self.eval_ops(builtin_values::values_value(receiver_value))
            }
            Builtin::Matches => {
                let pattern =
                    self.resolve_call_signature(builtin_calls::matches_pattern_arg(&args[0]))?;
                self.eval_ops(builtin_values::matches_value(receiver_value, pattern))
            }
        }
    }

    pub(super) fn resolve_call_signature<T>(
        &mut self,
        result: Result<T, CallSignatureError>,
    ) -> EvalResult<T> {
        match result {
            Ok(value) => Ok(value),
            Err(CallSignatureError::UnknownFunction { name }) => {
                self.diag(
                    CfdErrorCode::CheckEvalTypeError,
                    format!("未知函数 `{name}`"),
                );
                Err(EvalAbort::Error)
            }
            Err(CallSignatureError::Arity { message }) => {
                self.diag(CfdErrorCode::CheckEvalTypeError, message);
                Err(EvalAbort::Error)
            }
        }
    }

    pub(super) fn eval_min_max(
        &mut self,
        builtin: Builtin,
        args: &[CftSchemaCheckExpr],
    ) -> EvalResult<LocatedCheckValue> {
        let arg_value = self.eval_expr(&args[0])?;
        self.eval_min_max_value(builtin, arg_value)
    }

    pub(super) fn eval_min_max_value(
        &mut self,
        builtin: Builtin,
        arg_value: LocatedCheckValue,
    ) -> EvalResult<LocatedCheckValue> {
        self.charge_collection_work(&arg_value)?;
        let result =
            builtin_values::min_max_value(builtin, arg_value, self.model, &mut self.budget);
        self.eval_ops(result)
    }

    pub(super) fn eval_sum(
        &mut self,
        args: &[CftSchemaCheckExpr],
    ) -> EvalResult<LocatedCheckValue> {
        let arg_value = self.eval_expr(&args[0])?;
        self.eval_sum_value(arg_value)
    }

    pub(super) fn eval_sum_value(
        &mut self,
        arg_value: LocatedCheckValue,
    ) -> EvalResult<LocatedCheckValue> {
        self.charge_collection_work(&arg_value)?;
        let result = builtin_values::sum_value(arg_value, self.model, &mut self.budget);
        self.eval_ops(result)
    }

    pub(super) fn eval_unary(
        &mut self,
        op: CftSchemaUnaryOp,
        value: LocatedCheckValue,
    ) -> EvalResult<LocatedCheckValue> {
        self.eval_ops(ops::unary_op(self.schema, op, value))
    }

    #[allow(clippy::similar_names)]
    pub(super) fn eval_bin_op(
        &mut self,
        op: CftSchemaBinOp,
        lhs: &CftSchemaCheckExpr,
        rhs: &CftSchemaCheckExpr,
    ) -> EvalResult<LocatedCheckValue> {
        match op {
            CftSchemaBinOp::Or => {
                let lhs = self.eval_expr(lhs)?;
                let (lhs, lhs_path) = self.eval_ops(ops::expect_bool_operand(lhs, "左"))?;
                if lhs {
                    return Ok(LocatedCheckValue::new(CheckValue::Bool(true), lhs_path));
                }
                let rhs = self.eval_expr(rhs)?;
                let (rhs, rhs_path) = self.eval_ops(ops::expect_bool_operand(rhs, "右"))?;
                Ok(LocatedCheckValue::new(CheckValue::Bool(rhs), rhs_path))
            }
            CftSchemaBinOp::And => {
                let lhs = self.eval_expr(lhs)?;
                let (lhs, lhs_path) = self.eval_ops(ops::expect_bool_operand(lhs, "左"))?;
                if !lhs {
                    return Ok(LocatedCheckValue::new(CheckValue::Bool(false), lhs_path));
                }
                let rhs = self.eval_expr(rhs)?;
                let (rhs, rhs_path) = self.eval_ops(ops::expect_bool_operand(rhs, "右"))?;
                Ok(LocatedCheckValue::new(CheckValue::Bool(rhs), rhs_path))
            }
            _ => {
                let lhs = self.eval_expr(lhs)?;
                let rhs = self.eval_expr(rhs)?;
                let location = lhs.location.clone().or_else(|| rhs.location.clone());
                self.eval_ops(ops::eager_bin_op(
                    self.schema,
                    op,
                    lhs.value,
                    rhs.value,
                    location,
                ))
            }
        }
    }

    pub(super) fn diag(&mut self, code: CfdErrorCode, message: impl Into<String>) {
        self.diag_at(code, None, message);
    }

    pub(super) fn diag_at(
        &mut self,
        code: CfdErrorCode,
        location: Option<ValueLocation>,
        message: impl Into<String>,
    ) {
        let mut message = message.into();
        for context in &self.contexts {
            message.push_str("\n上下文: ");
            message.push_str(context);
        }
        self.diag_at_preformatted(code, location, message);
    }

    fn eval_unique(
        &mut self,
        collection: &CftSchemaCheckExpr,
        value: LocatedCheckValue,
    ) -> EvalResult<LocatedCheckValue> {
        self.charge_collection_work(&value)?;
        let result = builtin_values::unique_value(value, self.model, &mut self.budget);
        let evaluation = self.eval_ops(result)?;
        if let Some(duplicate) = evaluation.duplicate {
            self.note_unique_failure(collection, duplicate);
        }
        Ok(evaluation.value)
    }

    pub(super) fn diag_at_preformatted(
        &mut self,
        code: CfdErrorCode,
        location: Option<ValueLocation>,
        message: impl Into<String>,
    ) {
        let location = location.unwrap_or_else(|| self.check_origin.clone());
        let mut diagnostic = CfdDiagnostic::error(code, message.into())
            .with_primary(Some(location.blame.record), location.blame.path.clone());
        for reference in &location.references {
            diagnostic = diagnostic.with_related(
                Some(reference.record),
                reference.path.clone(),
                "referenced from here",
            );
        }
        if location.storage != location.blame && !location.references.contains(&location.storage) {
            diagnostic = diagnostic.with_related(
                Some(location.storage.record),
                location.storage.path,
                "value stored here",
            );
        }
        self.diagnostics.push(diagnostic);
    }
}
