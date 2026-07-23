use super::access;
use super::builtins::{self, Builtin, CallSignature, CallSignatureError, CallTarget};
use super::deps::DependencyCollector;
use super::diagnostics::{format_value_for_message, CheckDiagnostic, CheckDiagnosticContext};
use super::dimensions::{self, DimensionVariantAbort};
use super::evaluation_trace::EvaluationTrace;
use super::ops::{self, OpsResult};
use super::quantifiers;
use super::value::{EvalValue, LocatedEvalValue, ScalarValue, ValueLocation};
use coflow_cft::{CftSchema, CftSchemaBinOp, CftSchemaCheckExpr, CftSchemaCmpOp, CftSchemaUnaryOp};
use coflow_data_model::{CfdDataModel, CfdDiagnostic, CfdErrorCode, CfdRecordId};
use coflow_structure::{StructuralBudget, StructuralLimits, StructureKind, TraversalCursor};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use super::value::EvalRecordRef;

pub(super) struct CheckEvaluator<'model> {
    pub(super) schema: &'model CftSchema,
    pub(super) model: &'model CfdDataModel,
    pub(super) check_origin: Option<ValueLocation>,
    pub(super) current: EvalValue<'model>,
    pub(super) scopes: Vec<BTreeMap<String, LocatedEvalValue<'model>>>,
    pub(super) contexts: Vec<CheckDiagnosticContext>,
    pub(super) diagnostics: Vec<CheckDiagnostic>,
    deps: DependencyCollector,
    pub(super) dimension_round: Option<dimensions::DimensionRoundView>,
    trace: Option<EvaluationTrace>,
    regex_cache: Rc<RefCell<builtins::RegexCache>>,
    budget: StructuralBudget,
    eval_stack: Vec<TraversalCursor>,
    pub(super) schema_location: Option<crate::CheckSchemaLocation>,
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

impl<'model> CheckEvaluator<'model> {
    pub(super) fn new(
        schema: &'model CftSchema,
        model: &'model CfdDataModel,
        check_origin: Option<ValueLocation>,
        current: EvalValue<'model>,
        mut deps: DependencyCollector,
        regex_cache: Rc<RefCell<builtins::RegexCache>>,
        structural_limits: StructuralLimits,
    ) -> Self {
        let initial_top = match &current {
            EvalValue::Record(record) => record.top_record_id(),
            _ => None,
        };
        if let Some(record_id) = initial_top {
            deps.note_read_from(record_id, coflow_data_model::CfdPath::root());
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
            regex_cache,
            budget: StructuralBudget::new(structural_limits),
            eval_stack: Vec::new(),
            schema_location: None,
        }
    }

    pub(super) fn into_outputs(self) -> (Vec<CheckDiagnostic>, DependencyCollector) {
        (self.diagnostics, self.deps)
    }

    pub(super) fn note_read_from(&mut self, target: CfdRecordId, path: coflow_data_model::CfdPath) {
        self.deps.note_read_from(target, path);
    }

    fn note_value_read(&mut self, value: &LocatedEvalValue<'model>) {
        if matches!(&value.value, EvalValue::Record(record) if record.is_record_set_handle()) {
            return;
        }
        if let Some(location) = &value.location {
            self.note_read_from(location.storage.record, location.storage.path.clone());
        }
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
        record: &EvalRecordRef,
        field_name: &str,
        located: &mut LocatedEvalValue<'model>,
    ) -> EvalResult<()> {
        match dimensions::apply_dimension_variant(
            self.schema,
            self.model,
            self.dimension_round.as_ref(),
            record,
            field_name,
            located,
            &mut self.budget,
        ) {
            Ok(Some(record_id)) => {
                self.note_read_from(
                    record_id,
                    located.location.as_ref().map_or_else(
                        coflow_data_model::CfdPath::root,
                        |location| location.storage.path.clone(),
                    ),
                );
                Ok(())
            }
            Ok(None) => Ok(()),
            Err(DimensionVariantAbort::Skipped) => Err(EvalAbort::Skipped),
            Err(DimensionVariantAbort::Error {
                code,
                location,
                message,
            }) => {
                self.diag_at(code, *location, message);
                Err(EvalAbort::Error)
            }
        }
    }

    pub(super) fn eval_expr(
        &mut self,
        expr: &CftSchemaCheckExpr,
    ) -> EvalResult<LocatedEvalValue<'model>> {
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

    pub(super) fn charge_collection_work(
        &mut self,
        value: &LocatedEvalValue<'model>,
    ) -> EvalResult<()> {
        let work = value
            .value
            .collection_len()
            .map_or(0, |length| u64::try_from(length).unwrap_or(u64::MAX));
        self.charge_work_at(StructureKind::CheckEvaluation, work, value.location.clone())
    }

    pub(super) fn eval_expr_with_trace(
        &mut self,
        expr: &CftSchemaCheckExpr,
    ) -> (EvalResult<LocatedEvalValue<'model>>, EvaluationTrace) {
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

    pub(super) fn eval_name(&mut self, name: &str) -> EvalResult<LocatedEvalValue<'model>> {
        for scope in self.scopes.iter().rev() {
            if let Some(value) = scope.get(name) {
                let value = value.clone();
                self.note_value_read(&value);
                return Ok(value);
            }
        }
        let current_field = access::current_field(
            self.schema,
            self.model,
            &self.current,
            name,
            &mut self.budget,
        );
        if let Some(mut value) = self.eval_ops(current_field)? {
            if let EvalValue::Record(record) = self.current.clone() {
                self.apply_dimension_variant(&record, name, &mut value)?;
            }
            self.note_value_read(&value);
            return Ok(value);
        }
        if let Some(value) = self.schema.resolve_const(name) {
            return Ok(LocatedEvalValue::value(EvalValue::from_const(&value.value)));
        }
        if let Some(enum_meta) = self.schema.resolve_enum(name) {
            return Ok(LocatedEvalValue::value(EvalValue::EnumNamespace(
                enum_meta.name.clone(),
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
        target: LocatedEvalValue<'model>,
        name: &str,
    ) -> EvalResult<LocatedEvalValue<'model>> {
        let target_record = match &target.value {
            EvalValue::Record(record) => Some(record.clone()),
            _ => None,
        };
        let field = access::field_value(self.schema, self.model, target, name, &mut self.budget);
        let mut result = self.eval_ops(field)?;
        if let Some(record) = target_record {
            self.apply_dimension_variant(&record, name, &mut result)?;
        }
        self.note_value_read(&result);
        Ok(result)
    }

    pub(super) fn eval_index(
        &mut self,
        target: LocatedEvalValue<'model>,
        index: LocatedEvalValue<'model>,
    ) -> EvalResult<LocatedEvalValue<'model>> {
        let result = access::index_value(target, index, self.model, &mut self.budget);
        self.eval_ops(result)
    }

    pub(super) fn quantifier_item(
        &mut self,
        collection: &LocatedEvalValue<'model>,
        index: usize,
    ) -> EvalResult<Option<LocatedEvalValue<'model>>> {
        let result = quantifiers::quantifier_item(collection, index, self.model, &mut self.budget);
        self.eval_ops(result)
    }

    pub(super) fn eval_call(
        &mut self,
        name: &str,
        args: &[CftSchemaCheckExpr],
    ) -> EvalResult<LocatedEvalValue<'model>> {
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
                let Some(ScalarValue::Int(value)) = arg_value.value.scalar() else {
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
                let Some(enum_meta) = self.schema.resolve_enum(name) else {
                    return Err(EvalAbort::Error);
                };
                Ok(LocatedEvalValue::value(EvalValue::enum_value(
                    builtins::enum_with_value(self.schema, &enum_meta.name, value),
                )))
            }
            CallTarget::Builtin(builtin) => self.eval_builtin_call(builtin, args),
        }
    }

    pub(super) fn eval_builtin_call(
        &mut self,
        builtin: Builtin,
        args: &[CftSchemaCheckExpr],
    ) -> EvalResult<LocatedEvalValue<'model>> {
        match builtin {
            Builtin::Len => {
                let arg = &args[0];
                let arg_value = self.eval_expr(arg)?;
                self.eval_ops(builtins::len_value(arg_value))
            }
            Builtin::Contains => {
                let collection = self.eval_expr(&args[0])?;
                self.charge_collection_work(&collection)?;
                let value = self.eval_expr(&args[1])?;
                let contains = builtins::contains_value(
                    &collection,
                    &value.value,
                    self.model,
                    &mut self.budget,
                );
                Ok(LocatedEvalValue::new(
                    EvalValue::bool(self.eval_ops(contains)?),
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
                self.eval_ops(builtins::keys_value(arg_value))
            }
            Builtin::Values => {
                let arg = &args[0];
                let arg_value = self.eval_expr(arg)?;
                self.charge_collection_work(&arg_value)?;
                self.eval_ops(builtins::values_value(arg_value))
            }
            Builtin::Matches => {
                let value = self.eval_expr(&args[0])?;
                let pattern =
                    self.resolve_call_signature(builtins::matches_pattern_arg(&args[1]))?;
                let result = {
                    let mut cache = self.regex_cache.borrow_mut();
                    builtins::matches_value(value, pattern, &mut cache)
                };
                self.eval_ops(result)
            }
            Builtin::StartsWith
            | Builtin::EndsWith
            | Builtin::IsBlank
            | Builtin::Abs
            | Builtin::IsFinite
            | Builtin::ApproxEqual
            | Builtin::ContainsKey
            | Builtin::ContainsValue
            | Builtin::IsSorted
            | Builtin::IsStrictlySorted
            | Builtin::Intersects
            | Builtin::IsDisjoint
            | Builtin::IsSubsetOf
            | Builtin::IsSupersetOf => {
                let receiver_value = self.eval_expr(&args[0])?;
                self.eval_extended_builtin(builtin, receiver_value, &args[1..])
            }
        }
    }

    pub(super) fn eval_method_call(
        &mut self,
        receiver: &CftSchemaCheckExpr,
        name: &str,
        args: &[CftSchemaCheckExpr],
    ) -> EvalResult<LocatedEvalValue<'model>> {
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
            Builtin::Len => self.eval_ops(builtins::len_value(receiver_value)),
            Builtin::Contains => {
                self.charge_collection_work(&receiver_value)?;
                let value = self.eval_expr(&args[0])?;
                let contains = builtins::contains_value(
                    &receiver_value,
                    &value.value,
                    self.model,
                    &mut self.budget,
                );
                Ok(LocatedEvalValue::new(
                    EvalValue::bool(self.eval_ops(contains)?),
                    receiver_value.location.clone(),
                ))
            }
            Builtin::Unique => self.eval_unique(receiver, receiver_value),
            Builtin::Min | Builtin::Max => self.eval_min_max_value(builtin, &receiver_value),
            Builtin::Sum => self.eval_sum_value(receiver_value),
            Builtin::Keys => {
                self.charge_collection_work(&receiver_value)?;
                self.eval_ops(builtins::keys_value(receiver_value))
            }
            Builtin::Values => {
                self.charge_collection_work(&receiver_value)?;
                self.eval_ops(builtins::values_value(receiver_value))
            }
            Builtin::Matches => {
                let pattern =
                    self.resolve_call_signature(builtins::matches_pattern_arg(&args[0]))?;
                let result = {
                    let mut cache = self.regex_cache.borrow_mut();
                    builtins::matches_value(receiver_value, pattern, &mut cache)
                };
                self.eval_ops(result)
            }
            Builtin::StartsWith
            | Builtin::EndsWith
            | Builtin::IsBlank
            | Builtin::Abs
            | Builtin::IsFinite
            | Builtin::ApproxEqual
            | Builtin::ContainsKey
            | Builtin::ContainsValue
            | Builtin::IsSorted
            | Builtin::IsStrictlySorted
            | Builtin::Intersects
            | Builtin::IsDisjoint
            | Builtin::IsSubsetOf
            | Builtin::IsSupersetOf => self.eval_extended_builtin(builtin, receiver_value, args),
        }
    }

    fn eval_extended_builtin(
        &mut self,
        builtin: Builtin,
        receiver: LocatedEvalValue<'model>,
        args: &[CftSchemaCheckExpr],
    ) -> EvalResult<LocatedEvalValue<'model>> {
        match builtin {
            Builtin::StartsWith | Builtin::EndsWith => {
                self.charge_collection_work(&receiver)?;
                let argument = self.eval_expr(&args[0])?;
                self.eval_ops(builtins::string_predicate_value(
                    builtin,
                    receiver,
                    &argument.value,
                ))
            }
            Builtin::IsBlank => {
                self.charge_collection_work(&receiver)?;
                self.eval_ops(builtins::is_blank_value(receiver))
            }
            Builtin::Abs => self.eval_ops(builtins::abs_value(receiver)),
            Builtin::IsFinite => self.eval_ops(builtins::is_finite_value(receiver)),
            Builtin::ApproxEqual => {
                let other = self.eval_expr(&args[0])?;
                let epsilon = self.eval_expr(&args[1])?;
                self.eval_ops(builtins::approx_equal_value(
                    receiver,
                    &other.value,
                    &epsilon.value,
                ))
            }
            Builtin::ContainsKey | Builtin::ContainsValue => {
                self.charge_collection_work(&receiver)?;
                let value = self.eval_expr(&args[0])?;
                let result = if builtin == Builtin::ContainsKey {
                    builtins::contains_value(&receiver, &value.value, self.model, &mut self.budget)
                } else {
                    builtins::contains_value_in_dict(
                        &receiver,
                        &value.value,
                        self.model,
                        &mut self.budget,
                    )
                };
                Ok(LocatedEvalValue::new(
                    EvalValue::bool(self.eval_ops(result)?),
                    receiver.location.clone(),
                ))
            }
            Builtin::IsSorted | Builtin::IsStrictlySorted => {
                self.charge_collection_work(&receiver)?;
                let sorted = builtins::sorted_value(
                    &receiver,
                    self.model,
                    &mut self.budget,
                    builtin == Builtin::IsStrictlySorted,
                );
                Ok(LocatedEvalValue::new(
                    EvalValue::bool(self.eval_ops(sorted)?),
                    receiver.location.clone(),
                ))
            }
            Builtin::Intersects
            | Builtin::IsDisjoint
            | Builtin::IsSubsetOf
            | Builtin::IsSupersetOf => {
                self.charge_collection_work(&receiver)?;
                let other = self.eval_expr(&args[0])?;
                self.charge_collection_work(&other)?;
                let result = builtins::set_relation_value(
                    builtin,
                    &receiver,
                    &other,
                    self.model,
                    &mut self.budget,
                );
                Ok(LocatedEvalValue::new(
                    EvalValue::bool(self.eval_ops(result)?),
                    receiver.location.clone(),
                ))
            }
            _ => {
                self.diag(CfdErrorCode::CheckEvalTypeError, "invalid extended builtin");
                Err(EvalAbort::Error)
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
    ) -> EvalResult<LocatedEvalValue<'model>> {
        let arg_value = self.eval_expr(&args[0])?;
        self.eval_min_max_value(builtin, &arg_value)
    }

    pub(super) fn eval_min_max_value(
        &mut self,
        builtin: Builtin,
        arg_value: &LocatedEvalValue<'model>,
    ) -> EvalResult<LocatedEvalValue<'model>> {
        self.charge_collection_work(arg_value)?;
        let result = builtins::min_max_value(builtin, arg_value, self.model, &mut self.budget);
        self.eval_ops(result)
    }

    pub(super) fn eval_sum(
        &mut self,
        args: &[CftSchemaCheckExpr],
    ) -> EvalResult<LocatedEvalValue<'model>> {
        let arg_value = self.eval_expr(&args[0])?;
        self.eval_sum_value(arg_value)
    }

    pub(super) fn eval_sum_value(
        &mut self,
        arg_value: LocatedEvalValue<'model>,
    ) -> EvalResult<LocatedEvalValue<'model>> {
        self.charge_collection_work(&arg_value)?;
        let result = builtins::sum_value(arg_value, self.model, &mut self.budget);
        self.eval_ops(result)
    }

    pub(super) fn eval_unary(
        &mut self,
        op: CftSchemaUnaryOp,
        value: LocatedEvalValue<'model>,
    ) -> EvalResult<LocatedEvalValue<'model>> {
        self.eval_ops(ops::unary_op(self.schema, op, value))
    }

    #[allow(clippy::similar_names)]
    pub(super) fn eval_bin_op(
        &mut self,
        op: CftSchemaBinOp,
        lhs: &CftSchemaCheckExpr,
        rhs: &CftSchemaCheckExpr,
    ) -> EvalResult<LocatedEvalValue<'model>> {
        match op {
            CftSchemaBinOp::Or => {
                let lhs = self.eval_expr(lhs)?;
                let (lhs, lhs_path) = self.eval_ops(ops::expect_bool_operand(&lhs, "左"))?;
                if lhs {
                    return Ok(LocatedEvalValue::new(EvalValue::bool(true), lhs_path));
                }
                let rhs = self.eval_expr(rhs)?;
                let (rhs, rhs_path) = self.eval_ops(ops::expect_bool_operand(&rhs, "右"))?;
                Ok(LocatedEvalValue::new(EvalValue::bool(rhs), rhs_path))
            }
            CftSchemaBinOp::And => {
                let lhs = self.eval_expr(lhs)?;
                let (lhs, lhs_path) = self.eval_ops(ops::expect_bool_operand(&lhs, "左"))?;
                if !lhs {
                    return Ok(LocatedEvalValue::new(EvalValue::bool(false), lhs_path));
                }
                let rhs = self.eval_expr(rhs)?;
                let (rhs, rhs_path) = self.eval_ops(ops::expect_bool_operand(&rhs, "右"))?;
                Ok(LocatedEvalValue::new(EvalValue::bool(rhs), rhs_path))
            }
            _ => {
                let lhs = self.eval_expr(lhs)?;
                let rhs = self.eval_expr(rhs)?;
                let location = lhs.location.clone().or_else(|| rhs.location.clone());
                self.eval_ops(ops::eager_bin_op(
                    self.schema,
                    op,
                    &lhs.value,
                    &rhs.value,
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
        self.diag_at_preformatted(code, location, message);
    }

    fn eval_unique(
        &mut self,
        collection: &CftSchemaCheckExpr,
        value: LocatedEvalValue<'model>,
    ) -> EvalResult<LocatedEvalValue<'model>> {
        self.charge_collection_work(&value)?;
        let result = builtins::unique_value(value, self.model, &mut self.budget);
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
        let location = location.or_else(|| self.check_origin.clone());
        let mut diagnostic = CfdDiagnostic::error(code, message.into());
        if let Some(location) = location {
            diagnostic = diagnostic
                .with_primary(Some(location.blame.record), location.blame.path.clone());
            for reference in &location.references {
                diagnostic = diagnostic.with_related(
                    Some(reference.record),
                    reference.path.clone(),
                    "referenced from here",
                );
            }
            if location.storage != location.blame
                && !location.references.contains(&location.storage)
            {
                diagnostic = diagnostic.with_related(
                    Some(location.storage.record),
                    location.storage.path,
                    "value stored here",
                );
            }
        }
        self.diagnostics.push(CheckDiagnostic {
            diagnostic,
            contexts: self.contexts.clone(),
            is_custom_message: false,
            schema_location: self.schema_location.clone(),
        });
    }

    pub(super) fn diag_at_custom_message(
        &mut self,
        code: CfdErrorCode,
        location: Option<ValueLocation>,
        message: impl Into<String>,
    ) {
        self.diag_at_preformatted(code, location, message);
        if let Some(diagnostic) = self.diagnostics.last_mut() {
            diagnostic.is_custom_message = true;
        }
    }
}
