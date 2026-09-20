//! 显式检查入口；规则通过共享字节码运行时实际执行。
mod limits;
mod output;
pub use limits::EvaluationLimits;
pub use output::{CheckExecutionStats, CheckOutput};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CheckLimits {
    pub evaluation: EvaluationLimits,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckDiagnosticContext {
    Check {
        name: String,
    },
    When {
        expression: String,
    },
    Quantifier {
        kind: String,
        binding: String,
        item: String,
    },
    Dimension {
        dimension: String,
        variant: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckSchemaLocation {
    pub module: crate::schema::ModuleId,
    pub span: crate::source::Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckDiagnostic {
    pub diagnostic: crate::CfdDiagnostic,
    pub contexts: Vec<CheckDiagnosticContext>,
    pub schema_location: Option<CheckSchemaLocation>,
}

impl From<crate::CfdDiagnostic> for CheckDiagnostic {
    fn from(diagnostic: crate::CfdDiagnostic) -> Self {
        Self {
            diagnostic,
            contexts: Vec::new(),
            schema_location: None,
        }
    }
}

/// 项目工具的便捷入口；同一 Runtime 的宿主调用和细粒度选择使用 Runtime::run_checks。
pub fn execute_checks(
    schema: &crate::schema::CftSchema,
    model: &crate::CfdDataModel,
    limits: CheckLimits,
) -> CheckOutput {
    let contract = match crate::contract::Contract::new(schema.clone()) {
        Ok(contract) => contract,
        Err(error) => {
            return CheckOutput {
                request_diagnostics: vec![crate::CfdDiagnostic::error(
                    crate::CfdErrorCode::CheckEvalTypeError,
                    error.to_string(),
                )
                .into()],
                statistics: CheckExecutionStats {
                    requested_tasks: 1,
                    rejected_tasks: 1,
                    ..CheckExecutionStats::default()
                },
            }
        }
    };
    let runtime = crate::runtime::Runtime::from_model(
        std::sync::Arc::new(contract),
        model.clone(),
        crate::runtime::HostBindings::new(),
    )
    .map_err(|diagnostic| diagnostic.message);
    match runtime {
        Ok(runtime) => runtime.run_checks(crate::runtime::CheckSelection::default(), limits),
        Err(message) => CheckOutput {
            request_diagnostics: vec![crate::CfdDiagnostic::error(
                crate::CfdErrorCode::CheckEvalTypeError,
                message,
            )
            .into()],
            statistics: CheckExecutionStats {
                requested_tasks: 1,
                rejected_tasks: 1,
                ..CheckExecutionStats::default()
            },
        },
    }
}
