mod context;
mod evaluator;
mod expressions;
mod record_walker;
mod statements;

use crate::{
    CheckDiagnostic, CheckExecutionStats, CheckLimits, CheckOutput, CheckTarget, CheckTask,
    CheckTaskResult,
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
                request_diagnostics: vec![internal_diagnostic(format!(
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

    for task in tasks {
        if statistics.work_used >= limits.max_request_work {
            statistics.rejected_tasks += 1;
            results.push(CheckTaskResult {
                diagnostics: vec![internal_diagnostic(format!(
                    "check request work limit {} exceeded",
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
        let (diagnostics, work) = execute_task(
            schema,
            model,
            &task,
            prepared.get(&task.statement),
            task_limits,
            &regex_cache,
        );
        statistics.executed_tasks += 1;
        statistics.work_used = statistics.work_used.saturating_add(work).saturating_add(1);
        results.push(CheckTaskResult { task, diagnostics });
    }

    CheckOutput {
        results,
        request_diagnostics: Vec::new(),
        statistics,
    }
}

struct PreparedStatement<'schema> {
    statement: &'schema CftSchemaCheckStmt,
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
) -> (Vec<CheckDiagnostic>, u64) {
    let Some(prepared) = prepared else {
        return (vec![internal_diagnostic("unknown check statement")], 0);
    };
    let context = context::ExecutionContext::new(schema, model, evaluation_limits, regex_cache);
    match (&prepared.owner, task.target) {
        (PreparedOwner::Project(check), CheckTarget::Project) => {
            context.execute_project(check, prepared.statement)
        }
        (PreparedOwner::Type(owner), CheckTarget::Record(record)) => {
            let Some(target) = model.record(record) else {
                return (vec![internal_diagnostic("unknown check record target")], 0);
            };
            let direct = owner.name == *target.actual_type_name()
                || schema.check_owner_applies_to_actual(&owner.name, target.actual_type());
            let nested_host = schema
                .check_hosts_for_nested_type(&owner.name)
                .any(|host| host.as_str() == target.actual_type());
            if direct && !nested_host {
                context.execute_record(
                    owner,
                    value::ValueLocation::root(record),
                    prepared.statement,
                )
            } else {
                record_walker::RecordCheckWalker::new(context, record, owner)
                    .execute(prepared.statement)
            }
        }
        _ => (
            vec![internal_diagnostic(
                "check target does not match the statement owner",
            )],
            0,
        ),
    }
}

pub(super) fn internal_diagnostic(message: impl Into<String>) -> CheckDiagnostic {
    CfdDiagnostic::error(CfdErrorCode::CheckEvalTypeError, message.into()).into()
}
