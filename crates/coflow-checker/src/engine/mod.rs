mod context;
mod evaluator;
mod expressions;
mod statements;

use crate::{
    CheckDiagnostic, CheckExecutionStats, CheckLimits, CheckOutput, CheckTarget, CheckTask,
    CheckTaskResult, CheckTaskStatus,
};
use coflow_language::cft::{CftSchema, CftSchemaCheckStmt, CftTopLevelCheck, CftType, CheckOwner};
use coflow_model::{CfdDataModel, CfdDiagnostic, CfdErrorCode};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use crate::diagnostics;
use crate::diagnostics::{explanations, trace as evaluation_trace};
use crate::eval as value;
use crate::operations::{
    access, builtins, comparison as ops, predicates as type_predicates, quantifiers,
};

#[expect(
    clippy::too_many_lines,
    reason = "按语言或命令分支集中处理，保持分派与各分支语义可直接核对。"
)]
pub(crate) fn execute_tasks(
    schema: &CftSchema,
    model: &CfdDataModel,
    tasks: impl IntoIterator<Item = CheckTask>,
    limits: CheckLimits,
) -> CheckOutput {
    let mut unique = BTreeSet::new();
    for task in tasks {
        unique.insert(task);
        if unique.len() > limits.max_tasks {
            return CheckOutput {
                results: Vec::new(),
                request_diagnostics: vec![budget_diagnostic(format!(
                    "check task limit {} exceeded",
                    limits.max_tasks
                ))],
                statistics: CheckExecutionStats {
                    requested_tasks: unique.len(),
                    rejected_tasks: unique.len(),
                    ..CheckExecutionStats::default()
                },
            };
        }
    }
    let mut tasks = unique.into_iter().collect::<Vec<_>>();
    tasks.sort_by(|left, right| left.execution_cmp(right, schema));
    let prepared = tasks
        .iter()
        .map(|task| task.statement)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter_map(|task| {
            let statement = schema.check_statement(task)?;
            let owner = match &statement.info.owner {
                CheckOwner::Type(name) => PreparedOwner::Type(schema.resolve_type(name)?),
                CheckOwner::Project(name) => PreparedOwner::Project(schema.resolve_check(name)?),
            };
            Some((
                task,
                PreparedStatement {
                    statement: statement.statement,
                    info: statement.info,
                    owner,
                },
            ))
        })
        .collect::<BTreeMap<_, _>>();
    let mut statistics = CheckExecutionStats {
        requested_tasks: tasks.len(),
        ..CheckExecutionStats::default()
    };
    let regex_cache = RefCell::new(builtins::RegexCache::new());
    let mut results = Vec::with_capacity(tasks.len());

    let mut aborted_rules = BTreeSet::new();
    for task in tasks {
        let rule = prepared
            .get(&task.statement)
            .map(|statement| (task.target.clone(), statement.info.rule_id.clone()));
        if rule
            .as_ref()
            .is_some_and(|rule| aborted_rules.contains(rule))
        {
            statistics.rejected_tasks += 1;
            results.push(CheckTaskResult {
                task,
                diagnostics: Vec::new(),
                status: CheckTaskStatus::NotExecuted,
            });
            continue;
        }
        if statistics.work_used >= limits.max_request_work {
            statistics.rejected_tasks += 1;
            results.push(CheckTaskResult {
                status: CheckTaskStatus::NotExecuted,
                diagnostics: vec![budget_diagnostic(format!(
                    "check request work limit {} exceeded; task was not executed",
                    limits.max_request_work
                ))],
                task,
            });
            continue;
        }

        let remaining_work = limits
            .max_request_work
            .saturating_sub(statistics.work_used)
            .saturating_sub(1);
        let mut task_limits = limits.evaluation;
        task_limits.max_work = task_limits.max_work.min(remaining_work);
        let (diagnostics, work, triggered) = execute_task(
            schema,
            model,
            &task,
            prepared.get(&task.statement),
            task_limits,
            &regex_cache,
        );
        statistics.executed_tasks += 1;
        statistics.work_used = statistics.work_used.saturating_add(work).saturating_add(1);
        let status = if diagnostics
            .iter()
            .any(|diagnostic| diagnostic.diagnostic.code == CfdErrorCode::CheckBudgetExceeded)
        {
            CheckTaskStatus::BudgetExceeded
        } else if diagnostics
            .iter()
            .any(|diagnostic| !is_business_failure(diagnostic.diagnostic.code))
        {
            CheckTaskStatus::EvaluationError
        } else if !diagnostics.is_empty() {
            CheckTaskStatus::Failed
        } else if !triggered {
            CheckTaskStatus::NotTriggered
        } else {
            CheckTaskStatus::Passed
        };
        if matches!(
            status,
            CheckTaskStatus::EvaluationError | CheckTaskStatus::BudgetExceeded
        ) {
            if let Some(rule) = rule {
                aborted_rules.insert(rule);
            }
        }
        results.push(CheckTaskResult {
            status,
            task,
            diagnostics,
        });
    }

    CheckOutput {
        results,
        request_diagnostics: Vec::new(),
        statistics,
    }
}

struct PreparedStatement<'schema> {
    statement: &'schema CftSchemaCheckStmt,
    info: &'schema coflow_language::cft::CheckStatementInfo,
    owner: PreparedOwner<'schema>,
}

enum PreparedOwner<'schema> {
    Type(&'schema CftType),
    Project(&'schema CftTopLevelCheck),
}

fn execute_task(
    schema: &CftSchema,
    model: &CfdDataModel,
    task: &CheckTask,
    prepared: Option<&PreparedStatement<'_>>,
    evaluation_limits: crate::EvaluationLimits,
    regex_cache: &RefCell<builtins::RegexCache>,
) -> (Vec<CheckDiagnostic>, u64, bool) {
    let Some(prepared) = prepared else {
        return (
            vec![internal_diagnostic("unknown check statement")],
            0,
            true,
        );
    };
    let context = context::ExecutionContext::new(schema, model, evaluation_limits, regex_cache);
    match (&prepared.owner, &task.target) {
        (PreparedOwner::Project(check), CheckTarget::Project) => {
            context.execute_project(check, prepared.statement)
        }
        (PreparedOwner::Type(owner), CheckTarget::Record(record)) => {
            let Some(target) = model.record(*record) else {
                return (
                    vec![internal_diagnostic("unknown check record target")],
                    0,
                    true,
                );
            };
            let direct = owner.name == *target.actual_type_name()
                || schema.check_owner_applies_to_actual(&owner.name, target.actual_type());
            if direct {
                context.execute_record(
                    owner,
                    &owner.checks[prepared.info.block_index],
                    &prepared.info.rule_id,
                    value::ValueLocation::root(*record),
                    prepared.statement,
                )
            } else {
                (
                    vec![internal_diagnostic(
                        "record target does not match the statement owner",
                    )],
                    0,
                    true,
                )
            }
        }
        (
            PreparedOwner::Type(owner),
            CheckTarget::Data {
                record,
                path,
                dimension,
            },
        ) => {
            let mut location = value::ValueLocation::root(*record);
            location.storage.path = path.clone();
            if let Some((field, variant)) = dimension {
                location.storage = value::ModelCursor::dimension(*record, field, variant);
                location.storage.path = path.clone();
            }
            location.blame = location.storage.clone();
            let current = value::EvalRecordRef::Resolved(location.clone());
            if !current
                .actual_type(model)
                .is_some_and(|actual| schema.check_owner_applies_to_actual(&owner.name, actual))
            {
                return (
                    vec![internal_diagnostic(
                        "data target does not match the statement owner",
                    )],
                    0,
                    true,
                );
            }
            context.execute_record(
                owner,
                &owner.checks[prepared.info.block_index],
                &prepared.info.rule_id,
                location,
                prepared.statement,
            )
        }
        _ => (
            vec![internal_diagnostic(
                "check target does not match the statement owner",
            )],
            0,
            true,
        ),
    }
}

pub(super) fn internal_diagnostic(message: impl Into<String>) -> CheckDiagnostic {
    CfdDiagnostic::error(CfdErrorCode::CheckEvalTypeError, message.into()).into()
}

fn is_business_failure(code: CfdErrorCode) -> bool {
    matches!(
        code,
        CfdErrorCode::CheckFailed
            | CfdErrorCode::CheckBoolExpectedTrue
            | CfdErrorCode::CheckComparisonFailed
            | CfdErrorCode::CheckNegationFailed
            | CfdErrorCode::CheckAndFailed
            | CfdErrorCode::CheckOrFailed
            | CfdErrorCode::CheckTypePredicateFailed
            | CfdErrorCode::CheckContainsFailed
            | CfdErrorCode::CheckUniqueFailed
            | CfdErrorCode::CheckMatchesFailed
            | CfdErrorCode::CheckAllQuantifierFailed
            | CfdErrorCode::CheckAnyQuantifierFailed
            | CfdErrorCode::CheckNoneQuantifierFailed
    )
}

fn budget_diagnostic(message: impl Into<String>) -> CheckDiagnostic {
    CfdDiagnostic::error(CfdErrorCode::CheckBudgetExceeded, message.into()).into()
}
