use std::collections::BTreeMap;

use coflow_cft::{CftSchemaCheckExpr, CftSchemaCheckExprKind};
use coflow_data_model::{CfdErrorCode, CfdPath};

use super::diagnostics::{format_value_for_message, render_expr, CheckExplanation};
use super::value::{comparable_key, CheckValue, LocatedCheckValue};

pub(super) trait ValueExprEvaluator {
    fn eval_value_expr(&mut self, expr: &CftSchemaCheckExpr) -> Option<LocatedCheckValue>;
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

pub(super) fn value_expr_actual(
    evaluator: &mut impl ValueExprEvaluator,
    expr: &CftSchemaCheckExpr,
) -> String {
    evaluator.eval_value_expr(expr).map_or_else(
        || render_expr(expr),
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

    if let Some(value) = evaluator.eval_value_expr(collection) {
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
