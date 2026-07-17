use super::evaluator::{CheckEvaluator, EvalResult};
use super::ops;
use super::type_predicates;
use super::value::{EvalValue, LocatedEvalValue};
use coflow_cft::{CftSchemaCheckExpr, CftSchemaCheckExprKind};

pub(super) fn eval_expr<'model>(
    evaluator: &mut CheckEvaluator<'model>,
    expr: &CftSchemaCheckExpr,
) -> EvalResult<LocatedEvalValue<'model>> {
    match &expr.kind {
        CftSchemaCheckExprKind::Int(value) => Ok(LocatedEvalValue::value(EvalValue::int(*value))),
        CftSchemaCheckExprKind::Float(value) => {
            Ok(LocatedEvalValue::value(EvalValue::float(*value)))
        }
        CftSchemaCheckExprKind::Bool(value) => Ok(LocatedEvalValue::value(EvalValue::bool(*value))),
        CftSchemaCheckExprKind::Null => Ok(LocatedEvalValue::value(EvalValue::null())),
        CftSchemaCheckExprKind::String(value) => {
            Ok(LocatedEvalValue::value(EvalValue::string(value.clone())))
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

fn eval_field_expr<'model>(
    evaluator: &mut CheckEvaluator<'model>,
    inner: &CftSchemaCheckExpr,
    name: &str,
) -> EvalResult<LocatedEvalValue<'model>> {
    if let CftSchemaCheckExprKind::Name(enum_name) = &inner.kind {
        if let Some(enum_value) = evaluator.schema.enum_variant_value(enum_name, name) {
            if let Some(value) = evaluator.schema.enum_value_from_int(enum_name, enum_value) {
                return Ok(LocatedEvalValue::value(EvalValue::enum_value(value.into())));
            }
        }
    }
    let target = evaluator.eval_expr(inner)?;
    evaluator.eval_field(target, name)
}

fn eval_index_expr<'model>(
    evaluator: &mut CheckEvaluator<'model>,
    inner: &CftSchemaCheckExpr,
    index: &CftSchemaCheckExpr,
) -> EvalResult<LocatedEvalValue<'model>> {
    let target = evaluator.eval_expr(inner)?;
    let index = evaluator.eval_expr(index)?;
    let result = evaluator.eval_index(target, index)?;
    if let EvalValue::Record(record) = &result.value {
        if let Some(id) = record.top_record_id() {
            evaluator.note_read_from(id);
        }
    }
    Ok(result)
}

fn eval_is_expr<'model>(
    evaluator: &mut CheckEvaluator<'model>,
    inner: &CftSchemaCheckExpr,
    predicate: &coflow_cft::CftSchemaTypePredicate,
) -> EvalResult<LocatedEvalValue<'model>> {
    let value = evaluator.eval_expr(inner)?;
    Ok(LocatedEvalValue::new(
        EvalValue::bool(type_predicates::value_matches_predicate(
            evaluator.schema,
            evaluator.model,
            &value.value,
            predicate,
        )),
        value.location,
    ))
}

fn eval_cmp_chain_expr<'model>(
    evaluator: &mut CheckEvaluator<'model>,
    first: &CftSchemaCheckExpr,
    rest: &[(coflow_cft::CftSchemaCmpOp, CftSchemaCheckExpr)],
) -> EvalResult<LocatedEvalValue<'model>> {
    let mut lhs_expr = first;
    let mut lhs = evaluator.eval_expr(first)?;
    for (op, rhs_expr) in rest {
        let rhs = evaluator.eval_expr(rhs_expr)?;
        let location = lhs.location.clone().or_else(|| rhs.location.clone());
        if !evaluator.eval_ops(ops::compare(
            *op,
            &lhs.value,
            &rhs.value,
            rhs.location.clone(),
        ))? {
            evaluator.note_comparison_failure(lhs_expr, *op, rhs_expr, location.clone());
            return Ok(LocatedEvalValue::new(EvalValue::bool(false), location));
        }
        lhs_expr = rhs_expr;
        lhs = rhs;
    }
    Ok(LocatedEvalValue::value(EvalValue::bool(true)))
}
