use std::collections::BTreeMap;

use coflow_cft::{
    CftSchemaBinOp, CftSchemaCheckExpr, CftSchemaCheckExprKind, CftSchemaCmpOp, CftSchemaUnaryOp,
};
use coflow_data_model::{CfdErrorCode, CfdPath};

use super::diagnostics::{cmp_op_str, format_value_for_message, render_expr, CheckExplanation};
use super::evaluator::EvalResult;
use super::value::{comparable_key, CheckValue, LocatedCheckValue};

pub(super) trait ValueExprEvaluator {
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
