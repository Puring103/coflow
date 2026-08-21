mod codes;

pub use codes::{CftErrorCode, CftStage};

use crate::module::ModuleId;
use crate::syntax::Span;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CftDiagnostics {
    pub diagnostics: Vec<CftDiagnostic>,
}

impl CftDiagnostics {
    #[must_use]
    pub fn new(diagnostics: Vec<CftDiagnostic>) -> Self {
        Self { diagnostics }
    }

    #[must_use]
    pub fn one(diagnostic: CftDiagnostic) -> Self {
        Self {
            diagnostics: vec![diagnostic],
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.diagnostics.is_empty()
    }
}

impl From<Vec<CftDiagnostic>> for CftDiagnostics {
    fn from(diagnostics: Vec<CftDiagnostic>) -> Self {
        Self::new(diagnostics)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CftDiagnostic {
    pub code: CftErrorCode,
    pub stage: CftStage,
    pub severity: CftSeverity,
    pub message: String,
    pub primary: Option<CftLabel>,
    pub related: Vec<CftLabel>,
}

impl CftDiagnostic {
    #[must_use]
    pub fn error(
        code: CftErrorCode,
        module: impl Into<ModuleId>,
        span: Span,
        message: impl Into<String>,
    ) -> Self {
        Self {
            stage: code.stage(),
            severity: CftSeverity::Error,
            code,
            message: message.into(),
            primary: Some(CftLabel {
                module: module.into(),
                span,
                message: None,
            }),
            related: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_primary_message(mut self, message: impl Into<String>) -> Self {
        if let Some(primary) = &mut self.primary {
            primary.message = Some(message.into());
        }
        self
    }

    #[must_use]
    pub fn with_related(
        mut self,
        module: impl Into<ModuleId>,
        span: Span,
        message: impl Into<String>,
    ) -> Self {
        self.related.push(CftLabel {
            module: module.into(),
            span,
            message: Some(message.into()),
        });
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CftLabel {
    pub module: ModuleId,
    pub span: Span,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CftSeverity {
    Error,
}
