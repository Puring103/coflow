use super::evaluator::{CheckEvaluator, EvalResult};
use super::ops;
use super::type_predicates;
use super::value::{CheckRecordRef, CheckValue, LocatedCheckValue};
use coflow_cft::{CftSchemaCheckExpr, CftSchemaCheckExprKind};
use coflow_data_model::CfdEnumValue;

pub(super) fn eval_expr(
    evaluator: &mut CheckEvaluator<'_>,
    expr: &CftSchemaCheckExpr,
) -> EvalResult<LocatedCheckValue> {
    match &expr.kind {
        CftSchemaCheckExprKind::Int(value) => Ok(LocatedCheckValue::value(CheckValue::Int(*value))),
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
        CftSchemaCheckExprKind::Name(name) => evaluator.eval_name(name),
        CftSchemaCheckExprKind::Field { expr: inner, name } => {
            eval_field_expr(evaluator, inner, name)
        }
        CftSchemaCheckExprKind::Index { expr: inner, index } => {
            eval_index_expr(evaluator, inner, index)
        }
        CftSchemaCheckExprKind::Is {
            expr: inner,
            predicate,
        } => eval_is_expr(evaluator, inner, predicate),
        CftSchemaCheckExprKind::Call { name, args } => evaluator.eval_call(name, args),
        CftSchemaCheckExprKind::MethodCall {
            receiver,
            name,
            args,
        } => evaluator.eval_method_call(receiver, name, args),
        CftSchemaCheckExprKind::BinOp { op, lhs, rhs } => evaluator.eval_bin_op(*op, lhs, rhs),
        CftSchemaCheckExprKind::Unary { op, expr: inner } => {
            let value = evaluator.eval_expr(inner)?;
            evaluator.eval_unary(*op, value)
        }
        CftSchemaCheckExprKind::CmpChain { first, rest } => {
            eval_cmp_chain_expr(evaluator, first, rest)
        }
    }
}

fn eval_field_expr(
    evaluator: &mut CheckEvaluator<'_>,
    inner: &CftSchemaCheckExpr,
    name: &str,
) -> EvalResult<LocatedCheckValue> {
    if let CftSchemaCheckExprKind::Name(enum_name) = &inner.kind {
        if let Some(enum_value) = evaluator.schema.enum_variant_value(enum_name, name) {
            return Ok(LocatedCheckValue::value(CheckValue::Enum(CfdEnumValue {
                enum_name: enum_name.clone(),
                variant: Some(name.to_string()),
                value: enum_value,
            })));
        }
    }
    let target = evaluator.eval_expr(inner)?;
    evaluator.eval_field(target, name)
}

fn eval_index_expr(
    evaluator: &mut CheckEvaluator<'_>,
    inner: &CftSchemaCheckExpr,
    index: &CftSchemaCheckExpr,
) -> EvalResult<LocatedCheckValue> {
    let target = evaluator.eval_expr(inner)?;
    let index = evaluator.eval_expr(index)?;
    let result = evaluator.eval_index(target, index)?;
    if let CheckValue::Record(CheckRecordRef::Top(id)) = &result.value {
        evaluator.note_read_from(*id);
    }
    Ok(result)
}

fn eval_is_expr(
    evaluator: &mut CheckEvaluator<'_>,
    inner: &CftSchemaCheckExpr,
    predicate: &coflow_cft::CftSchemaTypePredicate,
) -> EvalResult<LocatedCheckValue> {
    let value = evaluator.eval_expr(inner)?;
    Ok(LocatedCheckValue::new(
        CheckValue::Bool(type_predicates::value_matches_predicate(
            evaluator.schema,
            evaluator.model,
            &value.value,
            predicate,
        )),
        value.path,
    ))
}

fn eval_cmp_chain_expr(
    evaluator: &mut CheckEvaluator<'_>,
    first: &CftSchemaCheckExpr,
    rest: &[(coflow_cft::CftSchemaCmpOp, CftSchemaCheckExpr)],
) -> EvalResult<LocatedCheckValue> {
    let mut lhs_expr = first;
    let mut lhs = evaluator.eval_expr(first)?;
    for (op, rhs_expr) in rest {
        let rhs = evaluator.eval_expr(rhs_expr)?;
        let path = lhs.path.clone().or_else(|| rhs.path.clone());
        if !evaluator.eval_ops(ops::compare(*op, &lhs.value, &rhs.value, rhs.path.clone()))? {
            evaluator.note_comparison_failure(lhs_expr, *op, rhs_expr, path.clone());
            return Ok(LocatedCheckValue::new(CheckValue::Bool(false), path));
        }
        lhs_expr = rhs_expr;
        lhs = rhs;
    }
    Ok(LocatedCheckValue::value(CheckValue::Bool(true)))
}
