use super::access;
use super::builtin_calls::{self, CallSignature, CallSignatureError, CallTarget};
use super::builtin_values;
use super::builtins::Builtin;
use super::deps::DependencyCollector;
use super::diagnostics::format_value_for_message;
use super::dimensions::{self, DimensionVariantAbort};
use super::enum_values;
use super::explanations::ValueExprEvaluator;
use super::fields;
use super::ops::{self, OpsResult};
use super::type_predicates;
use super::value::{CheckValue, LocatedCheckValue};
use crate::DimensionCheckContext;
use coflow_cft::{
    CftContainer, CftSchemaBinOp, CftSchemaCheckExpr, CftSchemaCheckExprKind, CftSchemaCmpOp,
    CftSchemaUnaryOp, CftSchemaView,
};
use coflow_data_model::{
    CfdDataModel, CfdDiagnostic, CfdEnumValue, CfdErrorCode, CfdPath, CfdRecordId,
};
use std::collections::BTreeMap;

use super::value::CheckRecordRef;

pub(super) struct CheckEvaluator<'a> {
    pub(super) schema: &'a CftSchemaView,
    pub(super) source_schema: &'a CftContainer,
    pub(super) model: &'a CfdDataModel,
    pub(super) root_record: Option<CfdRecordId>,
    pub(super) root_path: CfdPath,
    pub(super) current: CheckValue,
    pub(super) scopes: Vec<BTreeMap<String, LocatedCheckValue>>,
    pub(super) contexts: Vec<String>,
    pub(super) diagnostics: Vec<CfdDiagnostic>,
    deps: DependencyCollector,
    pub(super) dimension_context: Option<DimensionCheckContext>,
}

impl ValueExprEvaluator for CheckEvaluator<'_> {
    fn model(&self) -> &CfdDataModel {
        self.model
    }

    fn eval_value_expr(&mut self, expr: &CftSchemaCheckExpr) -> EvalResult<LocatedCheckValue> {
        self.eval_expr(expr)
    }

    fn eval_unary_expr(
        &mut self,
        op: CftSchemaUnaryOp,
        value: LocatedCheckValue,
    ) -> EvalResult<LocatedCheckValue> {
        self.eval_unary(op, value)
    }

    fn compare_values(
        &mut self,
        op: CftSchemaCmpOp,
        lhs: &CheckValue,
        rhs: &CheckValue,
        rhs_path: Option<CfdPath>,
    ) -> EvalResult<bool> {
        self.from_ops(ops::compare(op, lhs, rhs, rhs_path))
    }
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
        schema: &'a CftSchemaView,
        source_schema: &'a CftContainer,
        model: &'a CfdDataModel,
        root_record: Option<CfdRecordId>,
        root_path: CfdPath,
        current: CheckValue,
        mut deps: DependencyCollector,
    ) -> Self {
        let initial_top = match &current {
            CheckValue::Record(CheckRecordRef::Top(id)) => Some(*id),
            _ => None,
        };
        if let Some(record_id) = initial_top {
            deps.note_read_from(record_id);
        }
        Self {
            schema,
            source_schema,
            model,
            root_record,
            root_path,
            current,
            scopes: Vec::new(),
            contexts: Vec::new(),
            diagnostics: Vec::new(),
            deps,
            dimension_context: None,
        }
    }

    pub(super) fn into_outputs(self) -> (Vec<CfdDiagnostic>, DependencyCollector) {
        (self.diagnostics, self.deps)
    }

    pub(super) fn note_read_from(&mut self, target: CfdRecordId) {
        self.deps.note_read_from(target);
    }

    pub(super) fn from_ops<T>(&mut self, result: OpsResult<T>) -> EvalResult<T> {
        result.map_err(|err| {
            let (code, path, message) = err.into_parts();
            self.diag_at(code, path, message);
            EvalAbort::Error
        })
    }

    fn apply_dimension_variant(
        &mut self,
        record: &CheckRecordRef,
        field_name: &str,
        located: &mut LocatedCheckValue,
    ) -> EvalResult<()> {
        match dimensions::apply_dimension_variant(
            self.schema,
            self.source_schema,
            self.model,
            self.dimension_context.as_ref(),
            record,
            field_name,
            located,
        ) {
            Ok(Some(record_id)) => {
                self.note_read_from(record_id);
                Ok(())
            }
            Ok(None) => Ok(()),
            Err(DimensionVariantAbort::Skipped) => Err(EvalAbort::Skipped),
            Err(DimensionVariantAbort::Error {
                code,
                path,
                message,
            }) => {
                self.diag_at(code, path, message);
                Err(EvalAbort::Error)
            }
        }
    }

    #[allow(clippy::too_many_lines)]
    pub(super) fn eval_expr(&mut self, expr: &CftSchemaCheckExpr) -> EvalResult<LocatedCheckValue> {
        match &expr.kind {
            CftSchemaCheckExprKind::Int(value) => {
                Ok(LocatedCheckValue::value(CheckValue::Int(*value)))
            }
            CftSchemaCheckExprKind::Float(value) => {
                Ok(LocatedCheckValue::value(CheckValue::Float(*value)))
            }
            CftSchemaCheckExprKind::Bool(value) => {
                Ok(LocatedCheckValue::value(CheckValue::Bool(*value)))
            }
            CftSchemaCheckExprKind::Null => Ok(LocatedCheckValue::value(CheckValue::Null)),
            CftSchemaCheckExprKind::String(value) => {
                Ok(LocatedCheckValue::value(CheckValue::String(value.clone())))
            }
            CftSchemaCheckExprKind::Name(name) => self.eval_name(name),
            CftSchemaCheckExprKind::Field { expr: inner, name } => {
                if let CftSchemaCheckExprKind::Name(enum_name) = &inner.kind {
                    if let Some(enum_value) = self.schema.enum_variant_value(enum_name, name) {
                        return Ok(LocatedCheckValue::value(CheckValue::Enum(CfdEnumValue {
                            enum_name: enum_name.clone(),
                            variant: Some(name.clone()),
                            value: enum_value,
                        })));
                    }
                }
                let target = self.eval_expr(inner)?;
                self.eval_field(target, name)
            }
            CftSchemaCheckExprKind::Index { expr: inner, index } => {
                let target = self.eval_expr(inner)?;
                let index = self.eval_expr(index)?;
                let result = self.eval_index(target, index)?;
                if let CheckValue::Record(CheckRecordRef::Top(id)) = &result.value {
                    self.note_read_from(*id);
                }
                Ok(result)
            }
            CftSchemaCheckExprKind::Is {
                expr: inner,
                predicate,
            } => {
                let value = self.eval_expr(inner)?;
                Ok(LocatedCheckValue::new(
                    CheckValue::Bool(type_predicates::value_matches_predicate(
                        self.schema,
                        self.model,
                        &value.value,
                        predicate,
                    )),
                    value.path,
                ))
            }
            CftSchemaCheckExprKind::Call { name, args } => self.eval_call(name, args),
            CftSchemaCheckExprKind::MethodCall {
                receiver,
                name,
                args,
            } => self.eval_method_call(receiver, name, args),
            CftSchemaCheckExprKind::BinOp { op, lhs, rhs } => self.eval_bin_op(*op, lhs, rhs),
            CftSchemaCheckExprKind::Unary { op, expr: inner } => {
                let value = self.eval_expr(inner)?;
                self.eval_unary(*op, value)
            }
            CftSchemaCheckExprKind::CmpChain { first, rest } => {
                let mut lhs = self.eval_expr(first)?;
                for (op, rhs_expr) in rest {
                    let rhs = self.eval_expr(rhs_expr)?;
                    let path = lhs.path.clone().or_else(|| rhs.path.clone());
                    if !self.from_ops(ops::compare(
                        *op,
                        &lhs.value,
                        &rhs.value,
                        rhs.path.clone(),
                    ))? {
                        return Ok(LocatedCheckValue::new(CheckValue::Bool(false), path));
                    }
                    lhs = rhs;
                }
                Ok(LocatedCheckValue::value(CheckValue::Bool(true)))
            }
        }
    }

    fn eval_name(&mut self, name: &str) -> EvalResult<LocatedCheckValue> {
        for scope in self.scopes.iter().rev() {
            if let Some(value) = scope.get(name) {
                return Ok(value.clone());
            }
        }
        if let Some(mut value) = fields::current_field(
            self.schema,
            self.model,
            self.root_record,
            &self.root_path,
            &self.current,
            name,
        ) {
            if let CheckValue::Record(CheckRecordRef::Top(id)) = &value.value {
                self.note_read_from(*id);
            }
            if let CheckValue::Record(record) = self.current.clone() {
                self.apply_dimension_variant(&record, name, &mut value)?;
            }
            return Ok(value);
        }
        if let Some(value) = self.schema.consts.get(name) {
            return Ok(LocatedCheckValue::value(CheckValue::from_const(value)));
        }
        if self.schema.enums.contains_key(name) {
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

    fn eval_field(
        &mut self,
        target: LocatedCheckValue,
        name: &str,
    ) -> EvalResult<LocatedCheckValue> {
        let target_record = match &target.value {
            CheckValue::Record(record) => Some(record.clone()),
            _ => None,
        };
        let mut result = self.from_ops(fields::field_value(
            self.schema,
            self.model,
            self.root_record,
            &self.root_path,
            target,
            name,
        ))?;
        if let CheckValue::Record(CheckRecordRef::Top(id)) = &result.value {
            self.note_read_from(*id);
        }
        if let Some(record) = target_record {
            self.apply_dimension_variant(&record, name, &mut result)?;
        }
        Ok(result)
    }

    fn eval_index(
        &mut self,
        target: LocatedCheckValue,
        index: LocatedCheckValue,
    ) -> EvalResult<LocatedCheckValue> {
        self.from_ops(access::index_value(target, index))
    }

    fn eval_call(
        &mut self,
        name: &str,
        args: &[CftSchemaCheckExpr],
    ) -> EvalResult<LocatedCheckValue> {
        let signature = self.resolve_call_signature(CallSignature::resolve_function(
            name,
            args.len(),
            self.schema.enums.contains_key(name),
        ))?;

        match signature.target {
            CallTarget::EnumConstructor => {
                let arg = &args[0];
                let arg_value = self.eval_expr(arg)?;
                let arg_kind = arg_value.value.clone();
                let CheckValue::Int(value) = arg_value.value else {
                    self.diag_at(
                        CfdErrorCode::CheckEvalTypeError,
                        arg_value.path,
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

    fn eval_builtin_call(
        &mut self,
        builtin: Builtin,
        args: &[CftSchemaCheckExpr],
    ) -> EvalResult<LocatedCheckValue> {
        match builtin {
            Builtin::Len => {
                let arg = &args[0];
                let arg_value = self.eval_expr(arg)?;
                self.from_ops(builtin_values::len_value(arg_value))
            }
            Builtin::Contains => {
                let collection = self.eval_expr(&args[0])?;
                let value = self.eval_expr(&args[1])?;
                Ok(LocatedCheckValue::new(
                    CheckValue::Bool(
                        self.from_ops(builtin_values::contains_value(&collection, &value.value))?,
                    ),
                    collection.path.clone(),
                ))
            }
            Builtin::Unique => {
                let arg = &args[0];
                let arg_value = self.eval_expr(arg)?;
                self.from_ops(builtin_values::unique_value(arg_value))
            }
            Builtin::Min | Builtin::Max => self.eval_min_max(builtin, args),
            Builtin::Sum => self.eval_sum(args),
            Builtin::Keys => {
                let arg = &args[0];
                let arg_value = self.eval_expr(arg)?;
                self.from_ops(builtin_values::keys_value(arg_value))
            }
            Builtin::Values => {
                let arg = &args[0];
                let arg_value = self.eval_expr(arg)?;
                self.from_ops(builtin_values::values_value(arg_value))
            }
            Builtin::Matches => {
                let value = self.eval_expr(&args[0])?;
                let pattern =
                    self.resolve_call_signature(builtin_calls::matches_pattern_arg(&args[1]))?;
                self.from_ops(builtin_values::matches_value(value, pattern))
            }
        }
    }

    fn eval_method_call(
        &mut self,
        receiver: &CftSchemaCheckExpr,
        name: &str,
        args: &[CftSchemaCheckExpr],
    ) -> EvalResult<LocatedCheckValue> {
        let signature =
            self.resolve_call_signature(CallSignature::resolve_method(name, args.len()))?;
        let CallTarget::Builtin(builtin) = signature.target else {
            unreachable!("method calls cannot resolve to enum constructors");
        };

        let receiver_value = self.eval_expr(receiver)?;
        match builtin {
            Builtin::Len => self.from_ops(builtin_values::len_value(receiver_value)),
            Builtin::Contains => {
                let value = self.eval_expr(&args[0])?;
                Ok(LocatedCheckValue::new(
                    CheckValue::Bool(self.from_ops(builtin_values::contains_value(
                        &receiver_value,
                        &value.value,
                    ))?),
                    receiver_value.path.clone(),
                ))
            }
            Builtin::Unique => self.from_ops(builtin_values::unique_value(receiver_value)),
            Builtin::Min | Builtin::Max => self.eval_min_max_value(builtin, receiver_value),
            Builtin::Sum => self.eval_sum_value(receiver_value),
            Builtin::Keys => self.from_ops(builtin_values::keys_value(receiver_value)),
            Builtin::Values => self.from_ops(builtin_values::values_value(receiver_value)),
            Builtin::Matches => {
                let pattern =
                    self.resolve_call_signature(builtin_calls::matches_pattern_arg(&args[0]))?;
                self.from_ops(builtin_values::matches_value(receiver_value, pattern))
            }
        }
    }

    fn resolve_call_signature<T>(
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

    fn eval_min_max(
        &mut self,
        builtin: Builtin,
        args: &[CftSchemaCheckExpr],
    ) -> EvalResult<LocatedCheckValue> {
        let arg_value = self.eval_expr(&args[0])?;
        self.eval_min_max_value(builtin, arg_value)
    }

    fn eval_min_max_value(
        &mut self,
        builtin: Builtin,
        arg_value: LocatedCheckValue,
    ) -> EvalResult<LocatedCheckValue> {
        self.from_ops(builtin_values::min_max_value(builtin, arg_value))
    }

    fn eval_sum(&mut self, args: &[CftSchemaCheckExpr]) -> EvalResult<LocatedCheckValue> {
        let arg_value = self.eval_expr(&args[0])?;
        self.eval_sum_value(arg_value)
    }

    fn eval_sum_value(&mut self, arg_value: LocatedCheckValue) -> EvalResult<LocatedCheckValue> {
        self.from_ops(builtin_values::sum_value(arg_value))
    }

    fn eval_unary(
        &mut self,
        op: CftSchemaUnaryOp,
        value: LocatedCheckValue,
    ) -> EvalResult<LocatedCheckValue> {
        self.from_ops(ops::unary_op(self.schema, op, value))
    }

    #[allow(clippy::similar_names)]
    fn eval_bin_op(
        &mut self,
        op: CftSchemaBinOp,
        lhs: &CftSchemaCheckExpr,
        rhs: &CftSchemaCheckExpr,
    ) -> EvalResult<LocatedCheckValue> {
        match op {
            CftSchemaBinOp::Or => {
                let lhs = self.eval_expr(lhs)?;
                let (lhs, lhs_path) = self.from_ops(ops::expect_bool_operand(lhs, "左"))?;
                if lhs {
                    return Ok(LocatedCheckValue::new(CheckValue::Bool(true), lhs_path));
                }
                let rhs = self.eval_expr(rhs)?;
                let (rhs, rhs_path) = self.from_ops(ops::expect_bool_operand(rhs, "右"))?;
                Ok(LocatedCheckValue::new(CheckValue::Bool(rhs), rhs_path))
            }
            CftSchemaBinOp::And => {
                let lhs = self.eval_expr(lhs)?;
                let (lhs, lhs_path) = self.from_ops(ops::expect_bool_operand(lhs, "左"))?;
                if !lhs {
                    return Ok(LocatedCheckValue::new(CheckValue::Bool(false), lhs_path));
                }
                let rhs = self.eval_expr(rhs)?;
                let (rhs, rhs_path) = self.from_ops(ops::expect_bool_operand(rhs, "右"))?;
                Ok(LocatedCheckValue::new(CheckValue::Bool(rhs), rhs_path))
            }
            _ => {
                let lhs = self.eval_expr(lhs)?;
                let rhs = self.eval_expr(rhs)?;
                let path = lhs.path.clone().or_else(|| rhs.path.clone());
                self.from_ops(ops::eager_bin_op(
                    self.schema,
                    op,
                    lhs.value,
                    rhs.value,
                    path,
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
        path: Option<CfdPath>,
        message: impl Into<String>,
    ) {
        let path = match path {
            Some(path) => path,
            None => self.root_path.clone(),
        };
        let mut message = message.into();
        if !self.contexts.is_empty() && !message.contains("\n上下文: ") {
            for context in &self.contexts {
                message.push_str("\n上下文: ");
                message.push_str(context);
            }
        }
        self.diagnostics
            .push(CfdDiagnostic::error(code, message).with_primary(self.root_record, path));
    }
}
