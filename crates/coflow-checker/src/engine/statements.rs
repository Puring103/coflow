use super::diagnostics::{
    format_cfd_path_for_message, render_expr, render_stmt, CheckDiagnosticContext,
    CheckExplanation,
};
use super::evaluator::{CheckEvaluator, EvalAbort, EvalFlow};
use super::explanations;
use super::quantifiers;
use super::value::{LocatedEvalValue, ScalarValue, ValueLocation};
use coflow_cft::{
    CftSchemaCheckBlock, CftSchemaCheckExpr, CftSchemaCheckMessage, CftSchemaCheckMessageKind,
    CftSchemaCheckStmt, CftSchemaQuantifierKind, ScheduledCheckBlock,
};
use coflow_data_model::CfdErrorCode;
use coflow_structure::StructureKind;
use std::collections::BTreeMap;

pub(super) fn eval_check_block(
    evaluator: &mut CheckEvaluator<'_>,
    check: &CftSchemaCheckBlock,
) -> EvalFlow {
    eval_stmts(evaluator, &check.stmts)
}

pub(super) fn eval_scheduled_check_block(
    evaluator: &mut CheckEvaluator<'_>,
    scheduled: ScheduledCheckBlock<'_>,
) -> EvalFlow {
    let Some(indices) = scheduled.statement_indices() else {
        return eval_check_block(evaluator, scheduled.block());
    };
    let mut skipped = false;
    for index in indices {
        let Some(stmt) = scheduled.block().stmts.get(*index) else {
            continue;
        };
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
        CftSchemaCheckStmt::Expr {
            condition,
            message,
            ..
        } => eval_expr_stmt(
            evaluator,
            condition,
            message.as_ref(),
        ),
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

fn eval_expr_stmt(
    evaluator: &mut CheckEvaluator<'_>,
    expr: &CftSchemaCheckExpr,
    custom_message: Option<&CftSchemaCheckMessage>,
) -> EvalFlow {
    let (result, trace) = evaluator.eval_expr_with_trace(expr);
    match result {
        Ok(value) if matches!(value.value.scalar(), Some(ScalarValue::Bool(true))) => {
            EvalFlow::Continue
        }
        Ok(value) if matches!(value.value.scalar(), Some(ScalarValue::Bool(false))) => {
            let explanation = explanations::explain_false_expr(&trace, expr, &value)
                .unwrap_or_else(|| {
                    CheckExplanation::new(
                        CfdErrorCode::CheckFailed,
                        render_expr(expr),
                        value.location.clone(),
                    )
                });
            let message = match custom_message {
                Some(CftSchemaCheckMessage {
                    kind: CftSchemaCheckMessageKind::String(message),
                    ..
                }) => message.clone(),
                Some(CftSchemaCheckMessage {
                    kind: CftSchemaCheckMessageKind::Formatted(segments),
                    ..
                }) => match super::expressions::eval_formatted_segments(evaluator, segments) {
                    Ok(message) => message,
                    Err(EvalAbort::Skipped) => return EvalFlow::Skipped,
                    Err(EvalAbort::Error) => return EvalFlow::HardStop,
                },
                None => explanation.message(),
            };
            if custom_message.is_some() {
                evaluator.diag_at_custom_message(explanation.code, explanation.location, message);
            } else {
                evaluator.diag_at_preformatted(explanation.code, explanation.location, message);
            }
            EvalFlow::Continue
        }
        Ok(value) => {
            evaluator.diag_at(
                CfdErrorCode::CheckEvalTypeError,
                value.location,
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
        Ok(value) if matches!(value.value.scalar(), Some(ScalarValue::Bool(true))) => {
            evaluator
                .contexts
                .push(CheckDiagnosticContext::When {
                    expression: render_expr(condition),
                });
            let flow = eval_stmts(evaluator, body);
            let _ = evaluator.contexts.pop();
            flow
        }
        Ok(value) if matches!(value.value.scalar(), Some(ScalarValue::Bool(false))) => {
            EvalFlow::Continue
        }
        Ok(value) => {
            evaluator.diag_at(
                CfdErrorCode::CheckEvalTypeError,
                value.location,
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
    if evaluator.charge_collection_work(&collection_value).is_err() {
        return EvalFlow::HardStop;
    }
    let item_count = match evaluator.eval_ops(quantifiers::quantifier_len(&collection_value)) {
        Ok(item_count) => item_count,
        Err(EvalAbort::Skipped) => return EvalFlow::Skipped,
        Err(EvalAbort::Error) => return EvalFlow::HardStop,
    };
    eval_quantifier(
        evaluator,
        QuantifierExecution {
            kind,
            binding,
            collection_value: &collection_value,
            item_count,
            body,
            collection,
            stmt,
        },
    )
}

#[derive(Clone, Copy)]
struct QuantifierExecution<'a, 'model> {
    kind: CftSchemaQuantifierKind,
    binding: &'a str,
    collection_value: &'a LocatedEvalValue<'model>,
    item_count: usize,
    body: &'a [CftSchemaCheckStmt],
    collection: &'a CftSchemaCheckExpr,
    stmt: &'a CftSchemaCheckStmt,
}

fn eval_quantifier<'model>(
    evaluator: &mut CheckEvaluator<'model>,
    execution: QuantifierExecution<'_, 'model>,
) -> EvalFlow {
    let QuantifierExecution {
        kind,
        binding,
        collection_value,
        item_count,
        body,
        collection,
        stmt,
    } = execution;
    let quantifier_diagnostic_start = evaluator.diagnostics.len();
    let mut matched = 0_usize;
    let mut none_match_locations: Vec<Option<ValueLocation>> = Vec::new();
    let mut first_item_location = None;
    for index in 0..item_count {
        let item = match evaluator.quantifier_item(collection_value, index) {
            Ok(Some(item)) => item,
            Ok(None) => return EvalFlow::HardStop,
            Err(EvalAbort::Skipped) => return EvalFlow::Skipped,
            Err(EvalAbort::Error) => return EvalFlow::HardStop,
        };
        if first_item_location.is_none() {
            first_item_location.clone_from(&item.location);
        }
        if evaluator
            .charge_work_at(StructureKind::QuantifierIteration, 1, item.location.clone())
            .is_err()
        {
            return EvalFlow::HardStop;
        }
        let diagnostic_start = evaluator.diagnostics.len();
        let mut scope = BTreeMap::new();
        scope.insert(binding.to_string(), item.clone());
        evaluator.scopes.push(scope);
        let item_context = CheckDiagnosticContext::Quantifier {
            kind: match kind {
                CftSchemaQuantifierKind::All => "all",
                CftSchemaQuantifierKind::Any => "any",
                CftSchemaQuantifierKind::None => "none",
            }
            .to_string(),
            binding: binding.to_string(),
            item: item.location.as_ref().map_or_else(
                || render_expr(collection),
                |location| { format_cfd_path_for_message(&location.blame.path) }
            ),
        };
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
                let _ = evaluator.diagnostics.split_off(diagnostic_start);
            }
            CftSchemaQuantifierKind::None => {
                evaluator.diagnostics.truncate(diagnostic_start);
                if passed {
                    none_match_locations.push(item.location.clone());
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
        item_count,
        first_item_location,
        collection,
        stmt,
        quantifier_diagnostic_start,
        matched,
        none_match_locations,
    );
    EvalFlow::Continue
}

#[allow(clippy::too_many_arguments)]
fn finish_quantifier(
    evaluator: &mut CheckEvaluator<'_>,
    kind: CftSchemaQuantifierKind,
    item_count: usize,
    first_item_location: Option<ValueLocation>,
    collection: &CftSchemaCheckExpr,
    stmt: &CftSchemaCheckStmt,
    quantifier_diagnostic_start: usize,
    matched: usize,
    none_match_locations: Vec<Option<ValueLocation>>,
) {
    match kind {
        CftSchemaQuantifierKind::All => {
            rewrite_all_failures(evaluator, stmt, quantifier_diagnostic_start);
        }
        CftSchemaQuantifierKind::Any if matched == 0 => {
            emit_any_failure(evaluator, item_count, first_item_location, stmt);
        }
        CftSchemaQuantifierKind::Any => {}
        CftSchemaQuantifierKind::None if matched > 0 => {
            emit_none_failures(evaluator, collection, stmt, none_match_locations);
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
        if diagnostic.diagnostic.code != CfdErrorCode::CheckBoolExpectedTrue
            && diagnostic.diagnostic.code != CfdErrorCode::CheckComparisonFailed
            && diagnostic.diagnostic.code != CfdErrorCode::CheckNegationFailed
            && diagnostic.diagnostic.code != CfdErrorCode::CheckAndFailed
            && diagnostic.diagnostic.code != CfdErrorCode::CheckOrFailed
            && diagnostic.diagnostic.code != CfdErrorCode::CheckTypePredicateFailed
            && diagnostic.diagnostic.code != CfdErrorCode::CheckNullPredicateFailed
            && diagnostic.diagnostic.code != CfdErrorCode::CheckContainsFailed
            && diagnostic.diagnostic.code != CfdErrorCode::CheckUniqueFailed
            && diagnostic.diagnostic.code != CfdErrorCode::CheckMatchesFailed
            && diagnostic.diagnostic.code != CfdErrorCode::CheckFailed
        {
            continue;
        }
        diagnostic.diagnostic.code = CfdErrorCode::CheckAllQuantifierFailed;
        if !diagnostic.is_custom_message {
            diagnostic.diagnostic.message = format!(
                "校验失败: {}\n{}",
                render_stmt(stmt),
                diagnostic.diagnostic.message
            );
        }
    }
}

fn emit_any_failure(
    evaluator: &mut CheckEvaluator<'_>,
    item_count: usize,
    first_item_location: Option<ValueLocation>,
    stmt: &CftSchemaCheckStmt,
) {
    let explanation = CheckExplanation::new(
        CfdErrorCode::CheckAnyQuantifierFailed,
        render_stmt(stmt),
        first_item_location,
    )
    .with_actual(format!("0 / {item_count} 个元素匹配"))
    .with_expected("至少 1 个元素满足");
    evaluator.diag_at_preformatted(
        explanation.code,
        explanation.location.clone(),
        explanation.message(),
    );
}

fn emit_none_failures(
    evaluator: &mut CheckEvaluator<'_>,
    collection: &CftSchemaCheckExpr,
    stmt: &CftSchemaCheckStmt,
    none_match_locations: Vec<Option<ValueLocation>>,
) {
    for location in none_match_locations {
        let explanation = CheckExplanation::new(
            CfdErrorCode::CheckNoneQuantifierFailed,
            render_stmt(stmt),
            location.clone(),
        )
        .with_actual(format!(
            "{} 已匹配",
            location.as_ref().map_or_else(
                || render_expr(collection),
                |location| format_cfd_path_for_message(&location.blame.path),
            )
        ))
        .with_expected("没有元素满足");
        let message = explanation.message();
        evaluator.diag_at_preformatted(explanation.code, explanation.location, message);
    }
}
