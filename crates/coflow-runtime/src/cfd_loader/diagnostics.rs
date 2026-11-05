use crate::api::{Diagnostic, DiagnosticSet, Label, LineIndex, SourceLocation};
use crate::data_model::TextSpan;
#[cfg(test)]
use crate::data_model::{CfdDiagnostics, RecordOrigin};
#[cfg(test)]
use std::error::Error;
use std::fmt;
use std::path::Path;

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CfdTextLoadError {
    Text(CfdTextDiagnostics),
    DataModel {
        diagnostics: CfdDiagnostics,
        origins: Vec<RecordOrigin>,
    },
}

#[cfg(test)]
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

#[cfg(test)]
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

impl CfdTextErrorCode {
    pub const ALL: &[Self] = &[
        Self::Syntax,
        Self::UnknownType,
        Self::AbstractObjectType,
        Self::ObjectTypeMismatch,
        Self::UnknownField,
        Self::DuplicateField,
        Self::ReservedIdField,
        Self::TypeMismatch,
        Self::InvalidEnumVariant,
        Self::ReferenceNeedsMarker,
    ];
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Syntax => "CFD-TEXT-Syntax",
            Self::UnknownType => "CFD-TEXT-UnknownType",
            Self::AbstractObjectType => "CFD-TEXT-AbstractObjectType",
            Self::ObjectTypeMismatch => "CFD-TEXT-ObjectTypeMismatch",
            Self::UnknownField => "CFD-TEXT-UnknownField",
            Self::DuplicateField => "CFD-TEXT-DuplicateField",
            Self::ReservedIdField => "CFD-TEXT-ReservedIdField",
            Self::TypeMismatch => "CFD-TEXT-TypeMismatch",
            Self::InvalidEnumVariant => "CFD-TEXT-InvalidEnumVariant",
            Self::ReferenceNeedsMarker => "CFD-TEXT-ReferenceNeedsMarker",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CfdTextSpan {
    pub start: usize,
    pub end: usize,
}

pub(super) fn text_span(line_index: &LineIndex, source: &str, span: CfdTextSpan) -> TextSpan {
    let range = line_index.range(source, span.start, span.end);
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
    line_index: &LineIndex,
    diagnostics: CfdTextDiagnostics,
) -> DiagnosticSet {
    DiagnosticSet {
        publication: None,
        diagnostics: diagnostics
            .diagnostics
            .into_iter()
            .map(|diagnostic| {
                // 语法与降低错误共享本来源的行索引；错误密集时也不逐条重扫源码前缀。
                let range = line_index.range(source, diagnostic.span.start, diagnostic.span.end);
                Diagnostic::error(diagnostic.code.as_str(), "CFD", diagnostic.message).with_primary(
                    Label {
                        location: SourceLocation::FileSpan {
                            path: file.to_path_buf(),
                            start_line: range.start.line,
                            start_character: range.start.character,
                            end_line: range.end.line,
                            end_character: range.end.character,
                        },
                        message: None,
                    },
                )
            })
            .collect(),
    }
}
