use std::collections::BTreeMap;

use coflow_cft::{
    CftSchemaBinOp, CftSchemaCheckExpr, CftSchemaCheckExprKind, CftSchemaCmpOp, CftSchemaUnaryOp,
};
use coflow_data_model::{CfdDataModel, CfdErrorCode, CfdPath};

use super::diagnostics::{cmp_op_str, format_value_for_message, render_expr, CheckExplanation};
use super::evaluator::EvalResult;
use super::value::{comparable_key, CheckValue, LocatedCheckValue};

pub(super) trait ValueExprEvaluator {
    fn model(&self) -> &CfdDataModel;
    fn eval_value_expr(&mut self, expr: &CftSchemaCheckExpr) -> EvalResult<LocatedCheckValue>;
    fn eval_unary_expr(
        &mut self,
        op: CftSchemaUnaryOp,
        value: LocatedCheckValue,
    ) -> EvalResult<LocatedCheckValue>;
    fn compare_values(
        &mut self,
        op: CftSchemaCmpOp,
        lhs: &CheckValue,
        rhs: &CheckValue,
        rhs_path: Option<CfdPath>,
    ) -> EvalResult<bool>;
}

pub(super) fn eval_expr_explained(
    evaluator: &mut impl ValueExprEvaluator,
    expr: &CftSchemaCheckExpr,
) -> EvalResult<(LocatedCheckValue, Option<String>)> {
    match &expr.kind {
        CftSchemaCheckExprKind::CmpChain { first, rest } => {
            let mut lhs = evaluator.eval_value_expr(first)?;
            for (op, rhs_expr) in rest {
                let rhs = evaluator.eval_value_expr(rhs_expr)?;
                let path = lhs.path.clone().or_else(|| rhs.path.clone());
                if !evaluator.compare_values(*op, &lhs.value, &rhs.value, rhs.path.clone())? {
                    let detail = format!(
                        "{} {} {}",
                        format_value_for_message(&lhs.value),
                        cmp_op_str(*op),
                        format_value_for_message(&rhs.value),
                    );
                    return Ok((
                        LocatedCheckValue::new(CheckValue::Bool(false), path),
                        Some(detail),
                    ));
                }
                lhs = rhs;
            }
            Ok((LocatedCheckValue::value(CheckValue::Bool(true)), None))
        }
        CftSchemaCheckExprKind::Unary {
            op: CftSchemaUnaryOp::Not,
            expr: inner,
        } => {
            let inner_val = evaluator.eval_value_expr(inner)?;
            if matches!(inner_val.value, CheckValue::Bool(true)) {
                let detail = format!(
                    "期望 !{}，但内部表达式为 true",
                    format_value_for_message(&inner_val.value),
                );
                return Ok((
                    LocatedCheckValue::new(CheckValue::Bool(false), inner_val.path),
                    Some(detail),
                ));
            }
            evaluator
                .eval_unary_expr(CftSchemaUnaryOp::Not, inner_val)
                .map(|value| (value, None))
        }
        CftSchemaCheckExprKind::BinOp {
            op: CftSchemaBinOp::And,
            lhs,
            rhs,
        } => {
            let lhs_value = evaluator.eval_value_expr(lhs)?;
            if matches!(lhs_value.value, CheckValue::Bool(false)) {
                return Ok((
                    LocatedCheckValue::new(CheckValue::Bool(false), lhs_value.path),
                    Some("左侧条件为 false".to_string()),
                ));
            }
            let rhs_value = evaluator.eval_value_expr(rhs)?;
            if matches!(rhs_value.value, CheckValue::Bool(false)) {
                return Ok((
                    LocatedCheckValue::new(CheckValue::Bool(false), rhs_value.path),
                    Some("右侧条件为 false".to_string()),
                ));
            }
            let path = lhs_value.path.or(rhs_value.path);
            Ok((LocatedCheckValue::new(CheckValue::Bool(true), path), None))
        }
        _ => evaluator.eval_value_expr(expr).map(|value| (value, None)),
    }
}

pub(super) fn explain_false_value_expr(
    evaluator: &mut impl ValueExprEvaluator,
    expr: &CftSchemaCheckExpr,
    value: &LocatedCheckValue,
    rendered: String,
) -> CheckExplanation {
    match &expr.kind {
        CftSchemaCheckExprKind::Call { name, args } if name == "contains" && args.len() == 2 => {
            CheckExplanation::new(
                CfdErrorCode::CheckContainsFailed,
                rendered,
                value.path.clone(),
            )
            .with_actual(value_expr_actual(evaluator, &args[0]))
            .with_expected(format!("包含 {}", render_expr(&args[1])))
        }
        CftSchemaCheckExprKind::MethodCall {
            receiver,
            name,
            args,
        } if name == "contains" && args.len() == 1 => CheckExplanation::new(
            CfdErrorCode::CheckContainsFailed,
            rendered,
            value.path.clone(),
        )
        .with_actual(value_expr_actual(evaluator, receiver))
        .with_expected(format!("包含 {}", render_expr(&args[0]))),
        CftSchemaCheckExprKind::Call { name, args } if name == "isUnique" && args.len() == 1 => {
            unique_failed_explanation(evaluator, &rendered, &args[0], value.path.clone())
        }
        CftSchemaCheckExprKind::MethodCall {
            receiver,
            name,
            args,
        } if name == "isUnique" && args.is_empty() => {
            unique_failed_explanation(evaluator, &rendered, receiver, value.path.clone())
        }
        CftSchemaCheckExprKind::Call { name, args } if name == "matches" && args.len() == 2 => {
            CheckExplanation::new(
                CfdErrorCode::CheckMatchesFailed,
                rendered,
                value.path.clone(),
            )
            .with_actual(value_expr_actual(evaluator, &args[0]))
            .with_expected(format!("匹配 {}", render_expr(&args[1])))
        }
        CftSchemaCheckExprKind::MethodCall {
            receiver,
            name,
            args,
        } if name == "matches" && args.len() == 1 => CheckExplanation::new(
            CfdErrorCode::CheckMatchesFailed,
            rendered,
            value.path.clone(),
        )
        .with_actual(value_expr_actual(evaluator, receiver))
        .with_expected(format!("匹配 {}", render_expr(&args[0]))),
        _ => CheckExplanation::new(
            CfdErrorCode::CheckBoolExpectedTrue,
            rendered,
            value.path.clone(),
        )
        .with_actual("false")
        .with_expected("true"),
    }
}

pub(super) fn explain_false_expr(
    evaluator: &mut impl ValueExprEvaluator,
    expr: &CftSchemaCheckExpr,
    value: &LocatedCheckValue,
) -> Option<CheckExplanation> {
    let rendered = render_expr(expr);
    match &expr.kind {
        CftSchemaCheckExprKind::Name(name) => Some(
            CheckExplanation::new(
                CfdErrorCode::CheckBoolExpectedTrue,
                rendered,
                value.path.clone(),
            )
            .with_actual(format!("{name} = false"))
            .with_expected("true"),
        ),
        CftSchemaCheckExprKind::Bool(false) => Some(
            CheckExplanation::new(
                CfdErrorCode::CheckBoolExpectedTrue,
                rendered,
                value.path.clone(),
            )
            .with_actual("false")
            .with_expected("true"),
        ),
        CftSchemaCheckExprKind::Field { .. }
        | CftSchemaCheckExprKind::Index { .. }
        | CftSchemaCheckExprKind::Call { .. }
        | CftSchemaCheckExprKind::MethodCall { .. }
            if matches!(value.value, CheckValue::Bool(false)) =>
        {
            Some(explain_false_value_expr(evaluator, expr, value, rendered))
        }
        CftSchemaCheckExprKind::Unary {
            op: CftSchemaUnaryOp::Not,
            expr: inner,
        } => Some(
            CheckExplanation::new(
                CfdErrorCode::CheckNegationFailed,
                rendered,
                value.path.clone(),
            )
            .with_actual(format!("{} = true", render_expr(inner)))
            .with_expected("false"),
        ),
        CftSchemaCheckExprKind::BinOp {
            op: CftSchemaBinOp::And,
            lhs,
            rhs,
        } => {
            let left = evaluator.eval_value_expr(lhs).ok();
            let right = evaluator.eval_value_expr(rhs).ok();
            let failed = match (&left, &right) {
                (Some(left), Some(right))
                    if matches!(left.value, CheckValue::Bool(false))
                        && matches!(right.value, CheckValue::Bool(false)) =>
                {
                    format!("{} = false, {} = false", render_expr(lhs), render_expr(rhs))
                }
                (Some(left), _) if matches!(left.value, CheckValue::Bool(false)) => {
                    format!("{} = false", render_expr(lhs))
                }
                (_, Some(right)) if matches!(right.value, CheckValue::Bool(false)) => {
                    format!("{} = false", render_expr(rhs))
                }
                _ => "至少一个操作数为 false".to_string(),
            };
            Some(
                CheckExplanation::new(CfdErrorCode::CheckAndFailed, rendered, value.path.clone())
                    .with_actual(failed)
                    .with_expected("两侧都为 true"),
            )
        }
        CftSchemaCheckExprKind::BinOp {
            op: CftSchemaBinOp::Or,
            lhs,
            rhs,
        } => Some(
            CheckExplanation::new(CfdErrorCode::CheckOrFailed, rendered, value.path.clone())
                .with_actual(format!(
                    "{} = false, {} = false",
                    render_expr(lhs),
                    render_expr(rhs)
                ))
                .with_expected("至少一侧为 true"),
        ),
        CftSchemaCheckExprKind::Is {
            expr: inner,
            predicate,
        } => match predicate {
            coflow_cft::CftSchemaTypePredicate::Null => {
                let actual = evaluator.eval_value_expr(inner).ok().map_or_else(
                    || format!("{} 不是 null", render_expr(inner)),
                    |actual| {
                        format!(
                            "{} = {}",
                            render_expr(inner),
                            format_value_for_message(&actual.value)
                        )
                    },
                );
                Some(
                    CheckExplanation::new(
                        CfdErrorCode::CheckNullPredicateFailed,
                        rendered,
                        value.path.clone(),
                    )
                    .with_actual(actual)
                    .with_expected("null"),
                )
            }
            coflow_cft::CftSchemaTypePredicate::Type(type_name) => {
                let actual = evaluator
                    .eval_value_expr(inner)
                    .ok()
                    .and_then(|actual| {
                        actual
                            .value
                            .actual_type(evaluator.model())
                            .map(str::to_string)
                    })
                    .unwrap_or_else(|| "非对象".to_string());
                Some(
                    CheckExplanation::new(
                        CfdErrorCode::CheckTypePredicateFailed,
                        rendered,
                        value.path.clone(),
                    )
                    .with_actual(format!("实际类型 = {actual}"))
                    .with_expected(format!("类型为 {type_name}")),
                )
            }
        },
        CftSchemaCheckExprKind::CmpChain { first, rest } => {
            explain_failed_comparison(evaluator, &rendered, first, rest, value.path.clone())
        }
        _ => None,
    }
}

fn explain_failed_comparison(
    evaluator: &mut impl ValueExprEvaluator,
    rendered: &str,
    first: &CftSchemaCheckExpr,
    rest: &[(CftSchemaCmpOp, CftSchemaCheckExpr)],
    fallback_path: Option<CfdPath>,
) -> Option<CheckExplanation> {
    let mut lhs_expr = first;
    let mut lhs = evaluator.eval_value_expr(first).ok()?;
    for (op, rhs_expr) in rest {
        let rhs = evaluator.eval_value_expr(rhs_expr).ok()?;
        let path = lhs
            .path
            .clone()
            .or_else(|| rhs.path.clone())
            .or_else(|| fallback_path.clone());
        if !evaluator
            .compare_values(*op, &lhs.value, &rhs.value, rhs.path.clone())
            .ok()?
        {
            let null_predicate =
                matches!(lhs.value, CheckValue::Null) || matches!(rhs.value, CheckValue::Null);
            let code = if null_predicate && matches!(op, CftSchemaCmpOp::Eq | CftSchemaCmpOp::Ne) {
                CfdErrorCode::CheckNullPredicateFailed
            } else {
                CfdErrorCode::CheckComparisonFailed
            };
            let actual_expr = if lhs.path.is_some() {
                lhs_expr
            } else {
                rhs_expr
            };
            let actual_value = if lhs.path.is_some() {
                &lhs.value
            } else {
                &rhs.value
            };
            return Some(
                CheckExplanation::new(code, rendered.to_string(), path)
                    .with_actual(format!(
                        "{} = {}",
                        render_expr(actual_expr),
                        format_value_for_message(actual_value)
                    ))
                    .with_expected(format!("{} {}", cmp_op_str(*op), render_expr(rhs_expr))),
            );
        }
        lhs_expr = rhs_expr;
        lhs = rhs;
    }
    None
}

pub(super) fn value_expr_actual(
    evaluator: &mut impl ValueExprEvaluator,
    expr: &CftSchemaCheckExpr,
) -> String {
    evaluator.eval_value_expr(expr).map_or_else(
        |_| render_expr(expr),
        |value| {
            format!(
                "{} = {}",
                render_expr(expr),
                format_value_for_message(&value.value)
            )
        },
    )
}

fn unique_failed_explanation(
    evaluator: &mut impl ValueExprEvaluator,
    rendered: &str,
    collection: &CftSchemaCheckExpr,
    path: Option<CfdPath>,
) -> CheckExplanation {
    let mut explanation =
        CheckExplanation::new(CfdErrorCode::CheckUniqueFailed, rendered.to_string(), path)
            .with_actual(value_expr_actual(evaluator, collection))
            .with_expected("所有元素唯一");

    if let Ok(value) = evaluator.eval_value_expr(collection) {
        if let CheckValue::Array { items, .. } = value.value {
            let mut seen = BTreeMap::new();
            for (index, item) in items.iter().enumerate() {
                if let Some(key) = comparable_key(item) {
                    if let Some(first_index) = seen.insert(key, index) {
                        explanation = explanation.with_actual(format!(
                            "重复值 {} 出现在索引 {first_index} 和 {index}",
                            format_value_for_message(item)
                        ));
                        break;
                    }
                }
            }
        }
    }
    explanation
}
