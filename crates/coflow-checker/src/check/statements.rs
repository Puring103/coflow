use super::diagnostics::{
    format_cfd_path_for_message, one_line_message, render_expr, render_stmt, CheckExplanation,
};
use super::evaluator::{CheckEvaluator, EvalAbort, EvalFlow};
use super::explanations;
use super::quantifiers;
use super::value::{CheckValue, LocatedCheckValue};
use coflow_cft::{
    CftSchemaCheckBlock, CftSchemaCheckExpr, CftSchemaCheckStmt, CftSchemaQuantifierKind,
};
use coflow_data_model::{CfdErrorCode, CfdPath};
use std::collections::BTreeMap;

pub(super) fn eval_check_block(
    evaluator: &mut CheckEvaluator<'_>,
    check: &CftSchemaCheckBlock,
) -> EvalFlow {
    eval_stmts(evaluator, &check.stmts)
}

fn eval_stmts(evaluator: &mut CheckEvaluator<'_>, stmts: &[CftSchemaCheckStmt]) -> EvalFlow {
    let mut skipped = false;
    for stmt in stmts {
        match eval_stmt(evaluator, stmt) {
            EvalFlow::Continue => {}
            EvalFlow::Skipped => skipped = true,
            EvalFlow::HardStop => return EvalFlow::HardStop,
        }
    }
    if skipped {
        EvalFlow::Skipped
    } else {
        EvalFlow::Continue
    }
}

fn eval_stmt(evaluator: &mut CheckEvaluator<'_>, stmt: &CftSchemaCheckStmt) -> EvalFlow {
    match stmt {
        CftSchemaCheckStmt::Expr(expr) => eval_expr_stmt(evaluator, expr),
        CftSchemaCheckStmt::When {
            condition, body, ..
        } => eval_when_stmt(evaluator, condition, body),
        CftSchemaCheckStmt::Quantifier {
            kind,
            binding,
            collection,
            body,
            ..
        } => eval_quantifier_stmt(evaluator, *kind, binding, collection, body, stmt),
    }
}

fn eval_expr_stmt(evaluator: &mut CheckEvaluator<'_>, expr: &CftSchemaCheckExpr) -> EvalFlow {
    match explanations::eval_expr_explained(evaluator, expr) {
        Ok((value, _)) if matches!(value.value, CheckValue::Bool(true)) => EvalFlow::Continue,
        Ok((value, explanation)) if matches!(value.value, CheckValue::Bool(false)) => {
            let explanation = explanations::explain_false_expr(evaluator, expr, &value)
                .unwrap_or_else(|| {
                    let mut fallback = CheckExplanation::new(
                        CfdErrorCode::CheckFailed,
                        render_expr(expr),
                        value.path.clone(),
                    );
                    if let Some(detail) = explanation {
                        fallback = fallback.with_actual(detail);
                    }
                    fallback
                })
                .with_context(&evaluator.contexts);
            let message = explanation.message();
            evaluator.diag_at_preformatted(explanation.code, explanation.path, message);
            EvalFlow::Continue
        }
        Ok((value, _)) => {
            evaluator.diag_at(
                CfdErrorCode::CheckEvalTypeError,
                value.path,
                "check 表达式没有求值为 bool",
            );
            EvalFlow::HardStop
        }
        Err(EvalAbort::Skipped) => EvalFlow::Skipped,
        Err(EvalAbort::Error) => EvalFlow::HardStop,
    }
}

fn eval_when_stmt(
    evaluator: &mut CheckEvaluator<'_>,
    condition: &CftSchemaCheckExpr,
    body: &[CftSchemaCheckStmt],
) -> EvalFlow {
    match evaluator.eval_expr(condition) {
        Ok(value) if matches!(value.value, CheckValue::Bool(true)) => {
            evaluator
                .contexts
                .push(format!("在 when {} 内", render_expr(condition)));
            let flow = eval_stmts(evaluator, body);
            let _ = evaluator.contexts.pop();
            flow
        }
        Ok(value) if matches!(value.value, CheckValue::Bool(false)) => EvalFlow::Continue,
        Ok(value) => {
            evaluator.diag_at(
                CfdErrorCode::CheckEvalTypeError,
                value.path,
                "when 条件没有求值为 bool",
            );
            EvalFlow::HardStop
        }
        Err(EvalAbort::Skipped) => EvalFlow::Skipped,
        Err(EvalAbort::Error) => EvalFlow::HardStop,
    }
}

fn eval_quantifier_stmt(
    evaluator: &mut CheckEvaluator<'_>,
    kind: CftSchemaQuantifierKind,
    binding: &str,
    collection: &CftSchemaCheckExpr,
    body: &[CftSchemaCheckStmt],
    stmt: &CftSchemaCheckStmt,
) -> EvalFlow {
    let collection_value = match evaluator.eval_expr(collection) {
        Ok(value) => value,
        Err(EvalAbort::Skipped) => return EvalFlow::Skipped,
        Err(EvalAbort::Error) => return EvalFlow::HardStop,
    };
    let items = match evaluator.eval_ops(quantifiers::quantifier_items(collection_value)) {
        Ok(items) => items,
        Err(EvalAbort::Skipped) => return EvalFlow::Skipped,
        Err(EvalAbort::Error) => return EvalFlow::HardStop,
    };
    eval_quantifier(evaluator, kind, binding, &items, body, collection, stmt)
}

fn eval_quantifier(
    evaluator: &mut CheckEvaluator<'_>,
    kind: CftSchemaQuantifierKind,
    binding: &str,
    items: &[LocatedCheckValue],
    body: &[CftSchemaCheckStmt],
    collection: &CftSchemaCheckExpr,
    stmt: &CftSchemaCheckStmt,
) -> EvalFlow {
    let quantifier_diagnostic_start = evaluator.diagnostics.len();
    let mut matched = 0_usize;
    let mut any_failures = Vec::new();
    let mut none_match_paths: Vec<Option<CfdPath>> = Vec::new();
    for item in items {
        let diagnostic_start = evaluator.diagnostics.len();
        let mut scope = BTreeMap::new();
        scope.insert(binding.to_string(), item.clone());
        evaluator.scopes.push(scope);
        let item_context = format!(
            "绑定 {binding} 位于 {}",
            item.path
                .as_ref()
                .map_or_else(|| render_expr(collection), format_cfd_path_for_message)
        );
        evaluator.contexts.push(item_context);
        let flow = eval_stmts(evaluator, body);
        let passed = flow == EvalFlow::Continue && evaluator.diagnostics.len() == diagnostic_start;
        let _ = evaluator.contexts.pop();
        let _ = evaluator.scopes.pop();

        match flow {
            EvalFlow::Continue => {}
            EvalFlow::Skipped => {
                evaluator.diagnostics.truncate(diagnostic_start);
                continue;
            }
            EvalFlow::HardStop => return EvalFlow::HardStop,
        }

        match kind {
            CftSchemaQuantifierKind::All => {}
            CftSchemaQuantifierKind::Any => {
                let trial_failures = evaluator.diagnostics.split_off(diagnostic_start);
                if !passed {
                    any_failures.extend(trial_failures);
                }
            }
            CftSchemaQuantifierKind::None => {
                evaluator.diagnostics.truncate(diagnostic_start);
                if passed {
                    none_match_paths.push(item.path.clone());
                }
            }
        }

        if passed {
            matched += 1;
        }
    }

    finish_quantifier(
        evaluator,
        kind,
        items,
        collection,
        stmt,
        quantifier_diagnostic_start,
        matched,
        any_failures
            .first()
            .map(|diagnostic| diagnostic.message.as_str()),
        none_match_paths,
    );
    EvalFlow::Continue
}

#[allow(clippy::too_many_arguments)]
fn finish_quantifier(
    evaluator: &mut CheckEvaluator<'_>,
    kind: CftSchemaQuantifierKind,
    items: &[LocatedCheckValue],
    collection: &CftSchemaCheckExpr,
    stmt: &CftSchemaCheckStmt,
    quantifier_diagnostic_start: usize,
    matched: usize,
    first_any_failure: Option<&str>,
    none_match_paths: Vec<Option<CfdPath>>,
) {
    match kind {
        CftSchemaQuantifierKind::All => {
            rewrite_all_failures(evaluator, stmt, quantifier_diagnostic_start);
        }
        CftSchemaQuantifierKind::Any if matched == 0 => {
            emit_any_failure(evaluator, items, stmt, first_any_failure);
        }
        CftSchemaQuantifierKind::Any => {}
        CftSchemaQuantifierKind::None if matched > 0 => {
            emit_none_failures(evaluator, collection, stmt, none_match_paths);
        }
        CftSchemaQuantifierKind::None => {}
    }
}

fn rewrite_all_failures(
    evaluator: &mut CheckEvaluator<'_>,
    stmt: &CftSchemaCheckStmt,
    quantifier_diagnostic_start: usize,
) {
    for diagnostic in &mut evaluator.diagnostics[quantifier_diagnostic_start..] {
        if diagnostic.code != CfdErrorCode::CheckBoolExpectedTrue
            && diagnostic.code != CfdErrorCode::CheckComparisonFailed
            && diagnostic.code != CfdErrorCode::CheckNegationFailed
            && diagnostic.code != CfdErrorCode::CheckAndFailed
            && diagnostic.code != CfdErrorCode::CheckOrFailed
            && diagnostic.code != CfdErrorCode::CheckTypePredicateFailed
            && diagnostic.code != CfdErrorCode::CheckNullPredicateFailed
            && diagnostic.code != CfdErrorCode::CheckContainsFailed
            && diagnostic.code != CfdErrorCode::CheckUniqueFailed
            && diagnostic.code != CfdErrorCode::CheckMatchesFailed
            && diagnostic.code != CfdErrorCode::CheckFailed
        {
            continue;
        }
        diagnostic.code = CfdErrorCode::CheckAllQuantifierFailed;
        diagnostic.message = format!("校验失败: {}\n{}", render_stmt(stmt), diagnostic.message);
    }
}

fn emit_any_failure(
    evaluator: &mut CheckEvaluator<'_>,
    items: &[LocatedCheckValue],
    stmt: &CftSchemaCheckStmt,
    first_any_failure: Option<&str>,
) {
    let mut context = evaluator.contexts.clone();
    if let Some(message) = first_any_failure {
        context.push(format!("失败样例: {}", one_line_message(message)));
    }
    let explanation = CheckExplanation::new(
        CfdErrorCode::CheckAnyQuantifierFailed,
        render_stmt(stmt),
        items.first().and_then(|item| item.path.clone()),
    )
    .with_actual(format!("0 / {} 个元素匹配", items.len()))
    .with_expected("至少 1 个元素满足")
    .with_context(&context);
    evaluator.diag_at_preformatted(
        explanation.code,
        explanation.path.clone(),
        explanation.message(),
    );
}

fn emit_none_failures(
    evaluator: &mut CheckEvaluator<'_>,
    collection: &CftSchemaCheckExpr,
    stmt: &CftSchemaCheckStmt,
    none_match_paths: Vec<Option<CfdPath>>,
) {
    for path in none_match_paths {
        let explanation = CheckExplanation::new(
            CfdErrorCode::CheckNoneQuantifierFailed,
            render_stmt(stmt),
            path.clone(),
        )
        .with_actual(format!(
            "{} 已匹配",
            path.as_ref()
                .map_or_else(|| render_expr(collection), format_cfd_path_for_message)
        ))
        .with_expected("没有元素满足")
        .with_context(&evaluator.contexts);
        let message = explanation.message();
        evaluator.diag_at_preformatted(explanation.code, explanation.path, message);
    }
}
