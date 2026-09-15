//! 检查入口；当前版本只保留接口，不执行检查程序。
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

/// 空接口必须报告未实现，不能将未执行的检查报告为通过。
pub fn execute_checks(
    _schema: &crate::schema::CftSchema,
    _model: &crate::CfdDataModel,
    _limits: CheckLimits,
) -> CheckOutput {
    CheckOutput {
        request_diagnostics: vec![crate::CfdDiagnostic::error(
            crate::CfdErrorCode::ExecutionUnavailable,
            "当前版本未实现虚拟机与检查执行",
        )
        .into()],
        statistics: CheckExecutionStats {
            requested_tasks: 1,
            rejected_tasks: 1,
            ..CheckExecutionStats::default()
        },
    }
}
