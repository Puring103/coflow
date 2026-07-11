use coflow_cft::{
    CftSchemaBinOp, CftSchemaCheckExpr, CftSchemaCheckExprKind, CftSchemaUnaryOp,
};
use coflow_data_model::{CfdErrorCode, CfdPath};

use super::diagnostics::{cmp_op_str, render_expr, CheckExplanation};
use super::evaluation_trace::EvaluationTrace;
use super::value::{CheckValue, LocatedCheckValue};

pub(super) fn explain_false_value_expr(
    trace: &EvaluationTrace,
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
            .with_actual(value_expr_actual(trace, &args[0]))
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
        .with_actual(value_expr_actual(trace, receiver))
        .with_expected(format!("包含 {}", render_expr(&args[0]))),
        CftSchemaCheckExprKind::Call { name, args } if name == "isUnique" && args.len() == 1 => {
            unique_failed_explanation(trace, &rendered, &args[0], value.path.clone())
        }
        CftSchemaCheckExprKind::MethodCall {
            receiver,
            name,
            args,
        } if name == "isUnique" && args.is_empty() => {
            unique_failed_explanation(trace, &rendered, receiver, value.path.clone())
        }
        CftSchemaCheckExprKind::Call { name, args } if name == "matches" && args.len() == 2 => {
            CheckExplanation::new(
                CfdErrorCode::CheckMatchesFailed,
                rendered,
                value.path.clone(),
            )
            .with_actual(value_expr_actual(trace, &args[0]))
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
        .with_actual(value_expr_actual(trace, receiver))
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
    trace: &EvaluationTrace,
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
            Some(explain_false_value_expr(trace, expr, value, rendered))
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
            let left = trace.fact(lhs).and_then(|fact| fact.bool_value);
            let right = trace.fact(rhs).and_then(|fact| fact.bool_value);
            let failed = match (left, right) {
                (Some(false), Some(false)) => {
                    format!("{} = false, {} = false", render_expr(lhs), render_expr(rhs))
                }
                (Some(false), _) => {
                    format!("{} = false", render_expr(lhs))
                }
                (_, Some(false)) => {
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
                let actual = value_expr_actual(trace, inner);
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
                let actual = trace
                    .fact(inner)
                    .and_then(|fact| fact.actual_type.clone())
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
        CftSchemaCheckExprKind::CmpChain { .. } => {
            explain_failed_comparison(trace, &rendered, value.path.clone())
        }
        _ => None,
    }
}

fn explain_failed_comparison(
    trace: &EvaluationTrace,
    rendered: &str,
    fallback_path: Option<CfdPath>,
) -> Option<CheckExplanation> {
    let failure = trace.comparison_failure()?;
    let path = failure.path.clone().or(fallback_path);
    let null_predicate = failure.lhs.is_null || failure.rhs.is_null;
    let code = if null_predicate
        && matches!(
            failure.op,
            coflow_cft::CftSchemaCmpOp::Eq | coflow_cft::CftSchemaCmpOp::Ne
        ) {
        CfdErrorCode::CheckNullPredicateFailed
    } else {
        CfdErrorCode::CheckComparisonFailed
    };
    let (actual_expr, actual_value) = if failure.lhs.path.is_some() {
        (&failure.lhs_expression, failure.lhs.display.as_deref())
    } else {
        (&failure.rhs_expression, failure.rhs.display.as_deref())
    };
    Some(
        CheckExplanation::new(code, rendered.to_string(), path)
            .with_actual(format!(
                "{actual_expr} = {}",
                actual_value.unwrap_or("<unknown>")
            ))
            .with_expected(format!(
                "{} {}",
                cmp_op_str(failure.op),
                failure.rhs_expression
            )),
    )
}

pub(super) fn value_expr_actual(
    trace: &EvaluationTrace,
    expr: &CftSchemaCheckExpr,
) -> String {
    trace
        .fact(expr)
        .and_then(|fact| fact.display.as_ref())
        .map_or_else(
            || render_expr(expr),
            |display| format!("{} = {display}", render_expr(expr)),
        )
}

fn unique_failed_explanation(
    trace: &EvaluationTrace,
    rendered: &str,
    collection: &CftSchemaCheckExpr,
    path: Option<CfdPath>,
) -> CheckExplanation {
    let explanation =
        CheckExplanation::new(CfdErrorCode::CheckUniqueFailed, rendered.to_string(), path)
            .with_actual(value_expr_actual(trace, collection))
            .with_expected("所有元素唯一");
    match trace.unique_failure(collection) {
        Some(detail) => explanation.with_actual(detail),
        None => explanation,
    }
}
