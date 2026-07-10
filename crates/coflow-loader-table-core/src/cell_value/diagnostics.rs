#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellValueDiagnostics {
    pub diagnostics: Vec<CellValueDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellValueDiagnostic {
    pub code: CellValueErrorCode,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CellValueErrorCode {
    Syntax,
    InvalidDeclaredType,
    UnknownType,
    UnknownField,
    DuplicateField,
    MissingBoundary,
    TypeMismatch,
    ObjectTypeMismatch,
    AbstractObjectType,
    InvalidEnumVariant,
    MixedObjectStyle,
    StringNeedsQuotes,
    ReferenceNeedsMarker,
}

impl CellValueErrorCode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Syntax => "Syntax",
            Self::InvalidDeclaredType => "InvalidDeclaredType",
            Self::UnknownType => "UnknownType",
            Self::UnknownField => "UnknownField",
            Self::DuplicateField => "DuplicateField",
            Self::MissingBoundary => "MissingBoundary",
            Self::TypeMismatch => "TypeMismatch",
            Self::ObjectTypeMismatch => "ObjectTypeMismatch",
            Self::AbstractObjectType => "AbstractObjectType",
            Self::InvalidEnumVariant => "InvalidEnumVariant",
            Self::MixedObjectStyle => "MixedObjectStyle",
            Self::StringNeedsQuotes => "StringNeedsQuotes",
            Self::ReferenceNeedsMarker => "ReferenceNeedsMarker",
        }
    }
}

pub(super) fn syntax(message: impl Into<String>) -> CellValueDiagnostics {
    CellValueDiagnostics {
        diagnostics: vec![CellValueDiagnostic {
            code: CellValueErrorCode::Syntax,
            message: message.into(),
        }],
    }
}

pub(super) fn invalid_declared_type(message: impl Into<String>) -> CellValueDiagnostics {
    CellValueDiagnostics {
        diagnostics: vec![CellValueDiagnostic {
            code: CellValueErrorCode::InvalidDeclaredType,
            message: message.into(),
        }],
    }
}

pub(super) fn missing_boundary(message: impl Into<String>) -> CellValueDiagnostics {
    CellValueDiagnostics {
        diagnostics: vec![CellValueDiagnostic {
            code: CellValueErrorCode::MissingBoundary,
            message: message.into(),
        }],
    }
}

pub(super) fn type_mismatch(expected: &str) -> CellValueDiagnostics {
    CellValueDiagnostics {
        diagnostics: vec![CellValueDiagnostic {
            code: CellValueErrorCode::TypeMismatch,
            message: format!("expected {expected}"),
        }],
    }
}

pub(super) fn reference_needs_marker(text: &str) -> CellValueDiagnostics {
    CellValueDiagnostics {
        diagnostics: vec![CellValueDiagnostic {
            code: CellValueErrorCode::ReferenceNeedsMarker,
            message: format!(
                "record reference `{text}` must be written as `&{text}` in a reference-typed field"
            ),
        }],
    }
}
