use coflow_api::{byte_range, Diagnostic, DiagnosticSet, Label, SourceLocation};
use coflow_data_model::TextSpan;
use coflow_data_model::CfdDiagnostics;
use std::error::Error;
use std::fmt;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CfdTextLoadError {
    Text(CfdTextDiagnostics),
    DataModel(CfdDiagnostics),
}

impl fmt::Display for CfdTextLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Text(diagnostics) => diagnostics.fmt(f),
            Self::DataModel(diagnostics) => {
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
    pub(super) fn error(
        code: CfdTextErrorCode,
        message: impl Into<String>,
        span: CfdTextSpan,
    ) -> Self {
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

pub(super) fn text_span(source: &str, span: CfdTextSpan) -> TextSpan {
    let range = byte_range(source, span.start, span.end);
    TextSpan {
        start_line: range.start.line,
        start_character: range.start.character,
        end_line: range.end.line,
        end_character: range.end.character,
    }
}

pub(super) fn cfd_error_to_diagnostics(
    file: &Path,
    source: &str,
    err: CfdTextLoadError,
) -> DiagnosticSet {
    match err {
        CfdTextLoadError::Text(diagnostics) => DiagnosticSet {
            diagnostics: diagnostics
                .diagnostics
                .into_iter()
                .map(|diagnostic| {
                    let range = byte_range(source, diagnostic.span.start, diagnostic.span.end);
                    Diagnostic::error(
                        format!("CFD-TEXT-{:?}", diagnostic.code),
                        "CFD",
                        diagnostic.message,
                    )
                    .with_primary(Label {
                        location: SourceLocation::FileSpan {
                            path: file.to_path_buf(),
                            start_line: range.start.line,
                            start_character: range.start.character,
                            end_line: range.end.line,
                            end_character: range.end.character,
                        },
                        message: None,
                    })
                })
                .collect(),
        },
        CfdTextLoadError::DataModel(diagnostics) => DiagnosticSet {
            diagnostics: diagnostics
                .diagnostics
                .into_iter()
                .map(|diagnostic| {
                    Diagnostic::error(
                        diagnostic.code.as_str().to_string(),
                        diagnostic.stage.to_string(),
                        diagnostic.message,
                    )
                })
                .collect(),
        },
    }
}
