//! 检查只通过显式入口调度；每次请求实际执行所选程序，报告不参与 Runtime 可用性。
use super::*;
use crate::{
    check::{
        CheckDiagnostic, CheckDiagnosticContext, CheckLimits, CheckOutput, CheckSchemaLocation,
    },
    vm::{contract_programs::CheckProgram, executor::ExecutionLimits},
    CfdDiagnostic, CfdErrorCode,
};
use std::{collections::BTreeSet, sync::Mutex};

#[derive(Debug, Default)]
pub(super) struct CheckReporter {
    // 检查报告需要 Send+Sync（作为 HostService 绑定），保留互斥锁；仅在检查调度路径触碰。
    pub messages: Mutex<Vec<Vec<CheckMessage>>>,
    pub location: Mutex<Option<CheckSchemaLocation>>,
}
#[derive(Debug)]
pub(super) struct CheckMessage {
    pub text: String,
    pub location: Option<CheckSchemaLocation>,
}
impl HostService for CheckReporter {
    fn has_member(&self, field: &str, _: &CftValueType, _: &crate::schema::CftSchema) -> bool {
        field == "require"
    }
    fn read(&self, _: &str) -> Result<HostValue, ExecutionError> {
        Err(ExecutionError::InvalidAccess(
            "Check 服务没有数据字段".into(),
        ))
    }
    fn call(&self, field: &str, args: &[HostValue]) -> Result<HostValue, ExecutionError> {
        let [HostValue::Bool(condition), HostValue::String(message)] = args else {
            return Err(ExecutionError::InvalidAccess(
                "require 参数类型不匹配".into(),
            ));
        };
        if field != "require" {
            return Err(ExecutionError::InvalidAccess("未知 Check 服务函数".into()));
        }
        let mut messages = self
            .messages
            .lock()
            .map_err(|_| ExecutionError::RuntimeBusy)?;
        let report = messages.last_mut().ok_or_else(|| {
            ExecutionError::InvalidAccess(
                "require 报告接收器未绑定；普通调用可配置自定义 Host 服务".into(),
            )
        })?;
        if !condition {
            report.push(CheckMessage {
                text: message.clone(),
                location: self
                    .location
                    .lock()
                    .map_err(|_| ExecutionError::RuntimeBusy)?
                    .clone(),
            });
        }
        Ok(HostValue::None)
    }
}
#[derive(Debug, Clone)]
pub struct CheckSelection {
    pub records: Option<Vec<ValueId>>,
    pub names: BTreeSet<String>,
    pub include_global: bool,
}
impl Default for CheckSelection {
    fn default() -> Self {
        Self {
            records: None,
            names: BTreeSet::new(),
            include_global: true,
        }
    }
}
impl Runtime {
    pub fn run_checks(&self, selection: CheckSelection, limits: CheckLimits) -> CheckOutput {
        let mut output = CheckOutput::default();
        let _entry = match self.enter() {
            Ok(entry) => entry,
            Err(error) => {
                output.request_diagnostics.push(
                    CfdDiagnostic::error(CfdErrorCode::CheckEvalTypeError, error.to_string())
                        .into(),
                );
                return output;
            }
        };
        let records = selection
            .records
            .unwrap_or_else(|| self.records.values().copied().collect());
        let mut tasks: Vec<(CheckProgram<crate::vm::image::ValidatedProgram>, Option<ValueId>)> = Vec::new();
        for id in records {
            let value = match self.value(id) {
                Ok(value) => value,
                Err(error) => {
                    output.request_diagnostics.push(
                        CfdDiagnostic::error(CfdErrorCode::CheckEvalTypeError, error.to_string())
                            .into(),
                    );
                    continue;
                }
            };
            let Value::Object {
                type_name,
                key: Some(_),
                ..
            } = value.as_ref()
            else {
                output.request_diagnostics.push(
                    CfdDiagnostic::error(CfdErrorCode::CheckEvalTypeError, "check 目标必须是记录")
                        .into(),
                );
                continue;
            };
            let mut ancestors = self
                .contract
                .schema()
                .ancestor_type_names(type_name)
                .map(|names| {
                    names
                        .iter()
                        .rev()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            ancestors.push(type_name.clone());
            for owner in ancestors {
                for program in self.code().checks() {
                    if program.owner.as_deref() == Some(owner.as_str())
                        && (selection.names.is_empty() || selection.names.contains(&program.name))
                    {
                        tasks.push((program.clone(), Some(id)));
                    }
                }
            }
        }
        if selection.include_global {
            for program in self.code().checks() {
                if program.owner.is_none()
                    && (selection.names.is_empty() || selection.names.contains(&program.name))
                {
                    tasks.push((program.clone(), None));
                }
            }
        }
        output.statistics.requested_tasks = tasks.len();
        // 整个检查请求持有同一个执行链，Host 同步重入继续消耗外层预算。
        let host = match self.execution_host(ExecutionLimits {
            max_work: limits.evaluation.max_work,
            max_iterations: limits.evaluation.max_iterations,
            ..ExecutionLimits::default()
        }) {
            Ok(host) => host,
            Err(error) => {
                output.request_diagnostics.push(
                    CfdDiagnostic::error(CfdErrorCode::CheckEvalTypeError, error.to_string())
                        .into(),
                );
                return output;
            }
        };
        let budget = host.budget.clone();
        let starting_work = budget.remaining();
        for (program, owner) in tasks {
            if budget.remaining() == 0 {
                break;
            }
            if let Ok(mut reports) = self.check_reporter.messages.lock() {
                reports.push(Vec::new());
            }
            let result = self.execute_check_program(program.program.clone(), owner, budget.clone());
            let messages = self
                .check_reporter
                .messages
                .lock()
                .ok()
                .and_then(|mut reports| reports.pop())
                .unwrap_or_default();
            output.statistics.executed_tasks += 1;
            let diagnostic_start = output.request_diagnostics.len();
            for message in messages {
                let mut diagnostic = diagnostic(&program, CfdErrorCode::CheckFailed, message.text);
                if message.location.is_some() {
                    diagnostic.schema_location = message.location;
                }
                output.request_diagnostics.push(diagnostic);
            }
            if let Err(error) = result {
                let mut diagnostic = diagnostic(
                    &program,
                    if budget.remaining() == 0 {
                        CfdErrorCode::CheckBudgetExceeded
                    } else {
                        CfdErrorCode::CheckEvalTypeError
                    },
                    error.to_string(),
                );
                if let ExecutionError::Fault {
                    module: Some(module),
                    span,
                    ..
                } = &error
                {
                    diagnostic.schema_location = Some(CheckSchemaLocation {
                        module: module.clone(),
                        span: *span,
                    });
                }
                output.request_diagnostics.push(diagnostic);
            }
            if let Some(record) = owner.and_then(|id| self.check_record_ids.get(&id)).copied() {
                for diagnostic in &mut output.request_diagnostics[diagnostic_start..] {
                    diagnostic.diagnostic = diagnostic
                        .diagnostic
                        .clone()
                        .with_primary(Some(record), crate::CfdPath::default());
                }
            }
        }
        output.statistics.rejected_tasks =
            output.statistics.requested_tasks - output.statistics.executed_tasks;
        if output.statistics.rejected_tasks > 0
            && budget.remaining() == 0
            && !output
                .request_diagnostics
                .iter()
                .any(|diagnostic| diagnostic.diagnostic.code == CfdErrorCode::CheckBudgetExceeded)
        {
            output.request_diagnostics.push(
                CfdDiagnostic::error(
                    CfdErrorCode::CheckBudgetExceeded,
                    "检查总预算耗尽，检查未完成",
                )
                .into(),
            );
        }
        output.statistics.work_used = starting_work - budget.remaining();
        output
    }
}
fn diagnostic(program: &CheckProgram<crate::vm::image::ValidatedProgram>, code: CfdErrorCode, message: String) -> CheckDiagnostic {
    CheckDiagnostic {
        diagnostic: CfdDiagnostic::error(code, message),
        contexts: vec![CheckDiagnosticContext::Check {
            name: program.name.clone(),
        }],
        schema_location: Some(CheckSchemaLocation {
            module: program.module.clone(),
            span: program.program.spans.first().copied().unwrap_or_default(),
        }),
    }
}
