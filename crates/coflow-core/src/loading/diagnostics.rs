use crate::{CfdDiagnostics, RecordOrigin};
use std::{error::Error, fmt};
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CfdTextLoadError {
    Text(CfdTextDiagnostics),
    DataModel {
        diagnostics: CfdDiagnostics,
        origins: Vec<RecordOrigin>,
    },
}

impl fmt::Display for CfdTextLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Text(diagnostics) => diagnostics.fmt(f),
            Self::DataModel { diagnostics, .. } => {
                let first = diagnostics
                    .diagnostics
                    .first()
                    .map_or("data model error", |diagnostic| diagnostic.message.as_str());
                f.write_str(first)
            }
        }
    }
}

impl Error for CfdTextLoadError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CfdTextDiagnostics {
    pub diagnostics: Vec<CfdTextDiagnostic>,
}

impl CfdTextDiagnostics {
    #[must_use]
    pub fn one(diagnostic: CfdTextDiagnostic) -> Self {
        Self {
            diagnostics: vec![diagnostic],
        }
    }
}

impl fmt::Display for CfdTextDiagnostics {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let first = self
            .diagnostics
            .first()
            .map_or("CFD text error", |diagnostic| diagnostic.message.as_str());
        f.write_str(first)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CfdTextDiagnostic {
    pub code: CfdTextErrorCode,
    pub message: String,
    pub span: CfdTextSpan,
}

impl CfdTextDiagnostic {
    pub fn error(code: CfdTextErrorCode, message: impl Into<String>, span: CfdTextSpan) -> Self {
        Self {
            code,
            message: message.into(),
            span,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CfdTextErrorCode {
    Syntax,
    UnknownType,
    AbstractObjectType,
    ObjectTypeMismatch,
    UnknownField,
    DuplicateField,
    ReservedIdField,
    TypeMismatch,
    InvalidEnumVariant,
    ReferenceNeedsMarker,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CfdTextSpan {
    pub start: usize,
    pub end: usize,
}
