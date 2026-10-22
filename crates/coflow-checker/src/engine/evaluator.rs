use super::access;
use super::builtins::{self, Builtin, CallSignature, CallSignatureError, CallTarget};
use super::diagnostics::{format_value_for_message, CheckDiagnostic, CheckDiagnosticContext};
use super::evaluation_trace::EvaluationTrace;
use super::ops::{self, OpsResult};
use super::quantifiers;
use super::value::{EvalValue, LocatedEvalValue, ScalarValue, ValueLocation};
use crate::limits::{EvaluationBudget, EvaluationCursor, EvaluationKind};
use crate::EvaluationLimits;
use coflow_language::cft::{
    CftSchema, CftSchemaBinOp, CftSchemaCheckExpr, CftSchemaCmpOp, CftSchemaUnaryOp,
};
use coflow_model::{CfdDataModel, CfdDiagnostic, CfdErrorCode};
use std::cell::RefCell;
use std::collections::BTreeMap;

pub(super) struct CheckEvaluator<'model> {
    pub(super) schema: &'model CftSchema,
    pub(super) model: &'model CfdDataModel,
    pub(super) check_origin: Option<ValueLocation>,
    pub(super) current: EvalValue<'model>,
    pub(super) scopes: Vec<BTreeMap<String, LocatedEvalValue<'model>>>,
    pub(super) contexts: Vec<CheckDiagnosticContext>,
    pub(super) diagnostics: Vec<CheckDiagnostic>,
    pub(super) root_triggered: bool,
    trace: Option<EvaluationTrace>,
    regex_cache: &'model RefCell<builtins::RegexCache>,
    budget: EvaluationBudget,
    eval_stack: Vec<EvaluationCursor>,
    pub(super) schema_location: Option<crate::CheckSchemaLocation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EvalFlow {
    Continue,
    HardStop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EvalAbort {
    Error,
}

pub(super) type EvalResult<T> = Result<T, EvalAbort>;

impl<'model> CheckEvaluator<'model> {
    pub(super) fn new(
        schema: &'model CftSchema,
        model: &'model CfdDataModel,
        check_origin: Option<ValueLocation>,
        current: EvalValue<'model>,
        regex_cache: &'model RefCell<builtins::RegexCache>,
        evaluation_limits: EvaluationLimits,
    ) -> Self {
        Self {
            schema,
            model,
            check_origin,
            current,
            scopes: Vec::new(),
            contexts: Vec::new(),
            diagnostics: Vec::new(),
            root_triggered: true,
            trace: None,
            regex_cache,
            budget: EvaluationBudget::new(evaluation_limits),
            eval_stack: Vec::new(),
            schema_location: None,
        }
    }

    pub(super) fn into_execution(self) -> (Vec<CheckDiagnostic>, u64, bool) {
        (
            self.diagnostics,
            self.budget.work_used(),
            self.root_triggered,
        )
    }

    pub(super) fn eval_ops<T>(&mut self, result: OpsResult<T>) -> EvalResult<T> {
        result.map_err(|err| {
            let (code, location, message) = err.into_parts();
            self.diag_at(code, location, message);
            EvalAbort::Error
        })
    }

    pub(super) fn eval_expr(
        &mut self,
        expr: &CftSchemaCheckExpr,
    ) -> EvalResult<LocatedEvalValue<'model>> {
        self.eval_condition(expr)
            .map(|(value, _)| match value.value {
                EvalValue::Dimension { base, .. } => *base,
                _ => value,
            })
    }

    // 条件表达式拥有独立绑定作用域，只有成立的 && 链与 when 主体接收绑定。
    pub(super) fn eval_condition(
        &mut self,
        expr: &CftSchemaCheckExpr,
    ) -> EvalResult<(
        LocatedEvalValue<'model>,
        BTreeMap<String, LocatedEvalValue<'model>>,
    )> {
        let parent = self
            .eval_stack
            .last()
            .copied()
            .unwrap_or_else(EvaluationCursor::root);
        let cursor = match self.budget.enter(parent, EvaluationKind::Expression, 1) {
            Ok(cursor) => cursor,
            Err(error) => {
                self.diag(CfdErrorCode::CheckBudgetExceeded, error.to_string());
                return Err(EvalAbort::Error);
            }
        };
        self.eval_stack.push(cursor);
        let previous_span = self.schema_location.as_ref().map(|location| location.span);
        if let Some(location) = self.schema_location.as_mut() {
            location.span = expr.span;
        }
        self.scopes.push(BTreeMap::new());
        let result = super::expressions::eval_expr(self, expr);
        if let (Some(location), Some(span)) = (self.schema_location.as_mut(), previous_span) {
            location.span = span;
        }
        let mut bindings = self.scopes.pop().unwrap_or_default();
        let _ = self.eval_stack.pop();
        if let Ok(value) = &result {
            if let Some(trace) = &mut self.trace {
                trace.record(expr, value, self.model);
            }
        }
        result.map(|value| {
            if !matches!(value.value.scalar(), Some(ScalarValue::Bool(true))) {
                bindings.clear();
            }
            (value, bindings)
        })
    }

    pub(super) fn charge_work_at(
        &mut self,
        kind: EvaluationKind,
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

    pub(super) fn charge_nodes_at(
        &mut self,
        kind: EvaluationKind,
        nodes: u64,
        location: Option<ValueLocation>,
    ) -> EvalResult<()> {
        self.budget.charge_nodes(kind, nodes).map_err(|error| {
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
        self.charge_work_at(EvaluationKind::Expression, work, value.location.clone())
    }

    pub(super) fn charge_iteration_at(
        &mut self,
        location: Option<ValueLocation>,
    ) -> EvalResult<()> {
        self.budget.charge_iterations(1).map_err(|error| {
            self.diag_at(
                CfdErrorCode::CheckBudgetExceeded,
                location,
                error.to_string(),
            );
            EvalAbort::Error
        })
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
        if name == "None" {
            return Ok(LocatedEvalValue::value(EvalValue::Constant(
                coflow_language::cft::CftConstValue::OptionNone,
            )));
        }
        for scope in self.scopes.iter().rev() {
            if let Some(value) = scope.get(name) {
                let value = value.clone();
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
        if let Some(value) = self.eval_ops(current_field)? {
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
        let field = access::field_value(self.schema, self.model, target, name, &mut self.budget);
        self.eval_ops(field)
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
            CallTarget::Builtin(_) => {
                self.diag(
                    CfdErrorCode::CheckEvalTypeError,
                    "check built-ins require method syntax",
                );
                Err(EvalAbort::Error)
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

        let receiver_value = if matches!(
            builtin,
            Builtin::For | Builtin::Variants | Builtin::HasVariant
        ) {
            self.eval_condition(receiver)?.0
        } else {
            self.eval_expr(receiver)?
        };
        match builtin {
            Builtin::HasVariant => {
                let selected = self.eval_expr(&args[0])?;
                let Some(ScalarValue::String(variant)) = selected.value.scalar() else {
                    self.diag_at(
                        CfdErrorCode::CheckEvalTypeError,
                        selected.location,
                        "hasVariant() requires a string variant",
                    );
                    return Err(EvalAbort::Error);
                };
                let EvalValue::Dimension { base, field, .. } = receiver_value.value else {
                    self.diag_at(
                        CfdErrorCode::CheckEvalTypeError,
                        receiver_value.location,
                        "hasVariant() requires a dimension field",
                    );
                    return Err(EvalAbort::Error);
                };
                let present = base.location.as_ref().is_some_and(|location| {
                    self.model.has_dimension_variant(
                        location.storage.record,
                        field.as_str(),
                        variant,
                    )
                });
                Ok(LocatedEvalValue::new(
                    EvalValue::bool(present),
                    base.location,
                ))
            }
            Builtin::Variants => match receiver_value.value {
                EvalValue::Dimension {
                    base,
                    field,
                    dimension,
                    value_type,
                } => {
                    let Some(location) = base.location else {
                        return Err(EvalAbort::Error);
                    };
                    let variants = self
                        .model
                        .dimension_variants(dimension.as_str())
                        .cloned()
                        .collect::<Vec<_>>();
                    self.charge_nodes_at(
                        EvaluationKind::DataValue,
                        variants.len() as u64,
                        Some(location.clone()),
                    )?;
                    Ok(LocatedEvalValue::new(
                        EvalValue::Dict {
                            entries: crate::eval::EvalEntries::dimension(
                                location.clone(),
                                field,
                                variants,
                            ),
                            key_type: Some(coflow_language::cft::CftValueType::String),
                            value_type: Some(value_type),
                        },
                        Some(location),
                    ))
                }
                _ => {
                    self.diag_at(
                        CfdErrorCode::CheckEvalTypeError,
                        receiver_value.location,
                        "variants() requires a dimension field",
                    );
                    Err(EvalAbort::Error)
                }
            },
            Builtin::For => self.eval_dimension_for(receiver_value, &args[0]),
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

    fn eval_dimension_for(
        &mut self,
        receiver: LocatedEvalValue<'model>,
        variant: &CftSchemaCheckExpr,
    ) -> EvalResult<LocatedEvalValue<'model>> {
        let selected = self.eval_expr(variant)?;
        let Some(ScalarValue::String(variant)) = selected.value.scalar() else {
            self.diag_at(
                CfdErrorCode::CheckEvalTypeError,
                selected.location,
                "dimension variant must be string",
            );
            return Err(EvalAbort::Error);
        };
        let EvalValue::Dimension {
            base,
            field,
            value_type,
            ..
        } = receiver.value
        else {
            self.diag_at(
                CfdErrorCode::CheckEvalTypeError,
                receiver.location,
                "for() requires a dimension field",
            );
            return Err(EvalAbort::Error);
        };
        let Some(location) = base.location.as_ref() else {
            return Ok(*base);
        };
        let record_id = location.storage.record;
        let Some(overlay) = self
            .model
            .dimension_value(record_id, field.as_str(), variant)
        else {
            return Ok(*base);
        };
        // 显式 None 同样是覆盖；仅缺失覆盖才返回基础值。
        let storage = crate::eval::ModelCursor::dimension(record_id, field.as_str(), variant);
        let location = ValueLocation {
            storage: storage.clone(),
            blame: storage,
            references: location.references.clone(),
        };
        let value = EvalValue::from_cfd_value(
            overlay,
            Some(&value_type),
            location.clone(),
            self.model,
            &mut self.budget,
            EvaluationCursor::root(),
        );
        match value {
            Ok(value) => Ok(LocatedEvalValue::new(value, Some(location))),
            Err(error) => {
                self.diag_at(
                    CfdErrorCode::CheckBudgetExceeded,
                    *error.location,
                    error.error.to_string(),
                );
                Err(EvalAbort::Error)
            }
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

    pub(super) fn eval_min_max_value(
        &mut self,
        builtin: Builtin,
        arg_value: &LocatedEvalValue<'model>,
    ) -> EvalResult<LocatedEvalValue<'model>> {
        self.charge_collection_work(arg_value)?;
        let result = builtins::min_max_value(builtin, arg_value, self.model, &mut self.budget);
        self.eval_ops(result)
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
                let (lhs, bindings) = self.eval_condition(lhs)?;
                let (lhs, lhs_path) = self.eval_ops(ops::expect_bool_operand(&lhs, "左"))?;
                if !lhs {
                    return Ok(LocatedEvalValue::new(EvalValue::bool(false), lhs_path));
                }
                if let Some(scope) = self.scopes.last_mut() {
                    scope.extend(bindings);
                }
                let (rhs, bindings) = self.eval_condition(rhs)?;
                if let Some(scope) = self.scopes.last_mut() {
                    scope.extend(bindings);
                }
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

    pub(super) fn bind_some(
        &mut self,
        value: LocatedEvalValue<'model>,
        binding: &str,
    ) -> EvalResult<bool> {
        let inner = match value.value {
            EvalValue::Model(coflow_model::CfdValue::OptionSome(inner)) => {
                let Some(location) = value.location.clone() else {
                    self.diag(
                        CfdErrorCode::CheckEvalTypeError,
                        "Some value has no model location",
                    );
                    return Err(EvalAbort::Error);
                };
                let result = EvalValue::from_cfd_value(
                    inner,
                    None,
                    location,
                    self.model,
                    &mut self.budget,
                    EvaluationCursor::root(),
                );
                match result {
                    Ok(inner) => inner,
                    Err(error) => {
                        self.diag_at(
                            CfdErrorCode::CheckBudgetExceeded,
                            *error.location,
                            error.error.to_string(),
                        );
                        return Err(EvalAbort::Error);
                    }
                }
            }
            EvalValue::Constant(coflow_language::cft::CftConstValue::OptionSome(inner)) => {
                EvalValue::from_const(&inner)
            }
            EvalValue::Model(coflow_model::CfdValue::OptionNone)
            | EvalValue::Constant(coflow_language::cft::CftConstValue::OptionNone) => {
                return Ok(false)
            }
            _ => {
                self.diag_at(
                    CfdErrorCode::CheckEvalTypeError,
                    value.location,
                    "Some pattern requires an Option value",
                );
                return Err(EvalAbort::Error);
            }
        };
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(
                binding.to_string(),
                LocatedEvalValue::new(inner, value.location),
            );
        }
        Ok(true)
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
        let value_location = location.as_ref().map(|location| crate::CheckValueLocation {
            record: location.storage.record,
            path: location.storage.path.clone(),
            dimension: location
                .storage
                .dimension
                .as_ref()
                .map(|coordinate| (coordinate.field.clone(), coordinate.variant.clone())),
        });
        let mut diagnostic = CfdDiagnostic::error(code, message.into());
        let context_count = self.contexts.len();
        if let Some(location) = location {
            diagnostic.primary = Some(location.blame.diagnostic_label(self.model, None));
            for reference in &location.references {
                diagnostic.related.push(
                    reference
                        .diagnostic_label(self.model, Some("referenced from here".to_string())),
                );
            }
            if location.storage != location.blame
                && !location.references.contains(&location.storage)
            {
                diagnostic.related.push(
                    location
                        .storage
                        .diagnostic_label(self.model, Some("value stored here".to_string())),
                );
            }
            // 解引用后主位置位于目标记录，仍保留来源引用的维度上下文。
            let mut dimensions = std::collections::BTreeSet::new();
            for cursor in std::iter::once(&location.blame).chain(&location.references) {
                if let Some(coordinate) = &cursor.dimension {
                    if let Some(dimension) = self
                        .model
                        .record(cursor.record)
                        .and_then(|record| {
                            self.schema.field(record.actual_type(), &coordinate.field)
                        })
                        .and_then(|field| field.dimension.as_ref())
                    {
                        dimensions
                            .insert((dimension.dimension.to_string(), coordinate.variant.clone()));
                    }
                }
            }
            for (dimension, variant) in dimensions {
                self.contexts
                    .push(CheckDiagnosticContext::Dimension { dimension, variant });
            }
        }
        self.diagnostics.push(CheckDiagnostic {
            diagnostic,
            contexts: self.contexts.clone(),
            is_custom_message: false,
            schema_location: self.schema_location.clone(),
            value_location,
        });
        self.contexts.truncate(context_count);
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
