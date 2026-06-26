use crate::model::{CfdDictKey, CfdInputDictKey, CfdRecordId};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CfdDiagnostics {
    pub diagnostics: Vec<CfdDiagnostic>,
}

impl CfdDiagnostics {
    #[must_use]
    pub fn new(diagnostics: Vec<CfdDiagnostic>) -> Self {
        Self { diagnostics }
    }

    #[must_use]
    pub fn one(diagnostic: CfdDiagnostic) -> Self {
        Self {
            diagnostics: vec![diagnostic],
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.diagnostics.is_empty()
    }
}

impl From<CfdDiagnostic> for CfdDiagnostics {
    fn from(diagnostic: CfdDiagnostic) -> Self {
        Self::one(diagnostic)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CfdDiagnostic {
    pub code: CfdErrorCode,
    pub stage: CfdStage,
    pub severity: CfdSeverity,
    pub message: String,
    pub primary: Option<CfdLabel>,
    pub related: Vec<CfdLabel>,
}

impl CfdDiagnostic {
    #[must_use]
    pub fn error(code: CfdErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            stage: code.stage(),
            severity: CfdSeverity::Error,
            message: message.into(),
            primary: None,
            related: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_primary(mut self, record: Option<CfdRecordId>, path: CfdPath) -> Self {
        self.primary = Some(CfdLabel {
            record,
            path,
            message: None,
        });
        self
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
        record: Option<CfdRecordId>,
        path: CfdPath,
        message: impl Into<String>,
    ) -> Self {
        self.related.push(CfdLabel {
            record,
            path,
            message: Some(message.into()),
        });
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CfdLabel {
    pub record: Option<CfdRecordId>,
    pub path: CfdPath,
    pub message: Option<String>,
}

#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Default,
    serde::Serialize,
    serde::Deserialize,
)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
pub struct CfdPath {
    pub segments: Vec<CfdPathSegment>,
}

impl CfdPath {
    #[must_use]
    pub fn root() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn field(mut self, name: impl Into<String>) -> Self {
        self.segments.push(CfdPathSegment::Field(name.into()));
        self
    }

    #[must_use]
    pub fn index(mut self, index: usize) -> Self {
        self.segments.push(CfdPathSegment::Index(index));
        self
    }

    #[must_use]
    pub fn dict_key(mut self, key: impl Into<String>) -> Self {
        self.segments.push(CfdPathSegment::DictKey(key.into()));
        self
    }

    /// Append a dict-key segment using a validated [`CfdDictKey`].
    /// The display form preserves the key's actual type:
    /// - strings are shown quoted (e.g. `"foo"`),
    /// - ints are shown as numbers,
    /// - enum keys are shown as `Enum.Variant`.
    #[must_use]
    pub fn dict_key_value(mut self, key: &CfdDictKey) -> Self {
        self.segments
            .push(CfdPathSegment::DictKey(format_dict_key(key)));
        self
    }

    /// Append a dict-key segment using an unvalidated [`CfdInputDictKey`].
    /// Used for diagnostics emitted before key validation succeeds.
    #[must_use]
    pub fn dict_key_input(mut self, key: &CfdInputDictKey) -> Self {
        self.segments
            .push(CfdPathSegment::DictKey(format_input_dict_key(key)));
        self
    }
}

#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum CfdPathSegment {
    Field(String),
    Index(usize),
    DictKey(String),
}

fn format_dict_key(key: &CfdDictKey) -> String {
    match key {
        CfdDictKey::String(value) => format!("\"{value}\""),
        CfdDictKey::Int(value) => value.to_string(),
        CfdDictKey::Enum(value) => value.variant.as_deref().map_or_else(
            || format!("{}({})", value.enum_name, value.value),
            |variant| format!("{}.{}", value.enum_name, variant),
        ),
    }
}

fn format_input_dict_key(key: &CfdInputDictKey) -> String {
    match key {
        CfdInputDictKey::String(value) => format!("\"{value}\""),
        CfdInputDictKey::Int(value) => value.to_string(),
        CfdInputDictKey::EnumVariant { enum_name, variant } => {
            format!("{enum_name}.{variant}")
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CfdStage {
    DataModel,
    Reference,
    Check,
}

impl fmt::Display for CfdStage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::DataModel => "DATA",
            Self::Reference => "REF",
            Self::Check => "CHECK",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CfdSeverity {
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CfdErrorCode {
    UnknownType,
    AbstractRecordType,
    MissingObjectType,
    ObjectTypeMismatch,
    UnknownField,
    MissingRequiredField,
    TypeMismatch,
    InvalidEnumVariant,
    DuplicateDictKey,
    MissingIdField,
    InvalidRecordKey,
    DuplicateId,
    DuplicatePolymorphicId,
    SingletonRecordCountInvalid,
    SingletonKeyMissingOrInvalid,
    SingletonKeyCollision,
    RefTargetNotFound,
    CheckFailed,
    CheckEvalTypeError,
    CheckNullAccess,
    CheckIndexOutOfBounds,
    CheckMissingDictKey,
    CheckEmptyMinMax,
    CheckComparisonFailed,
    CheckBoolExpectedTrue,
    CheckNegationFailed,
    CheckAndFailed,
    CheckOrFailed,
    CheckTypePredicateFailed,
    CheckNullPredicateFailed,
    CheckContainsFailed,
    CheckUniqueFailed,
    CheckMatchesFailed,
    CheckAnyQuantifierFailed,
    CheckNoneQuantifierFailed,
    CheckAllQuantifierFailed,
}

impl CfdErrorCode {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UnknownType => "CFD-DATA-001",
            Self::AbstractRecordType => "CFD-DATA-002",
            Self::MissingObjectType => "CFD-DATA-003",
            Self::ObjectTypeMismatch => "CFD-DATA-004",
            Self::UnknownField => "CFD-DATA-005",
            Self::MissingRequiredField => "CFD-DATA-006",
            Self::TypeMismatch => "CFD-DATA-007",
            Self::InvalidEnumVariant => "CFD-DATA-008",
            Self::DuplicateDictKey => "CFD-DATA-009",
            Self::MissingIdField => "CFD-DATA-010",
            Self::DuplicateId => "CFD-DATA-011",
            Self::DuplicatePolymorphicId => "CFD-DATA-012",
            Self::InvalidRecordKey => "CFD-DATA-013",
            Self::SingletonRecordCountInvalid => "CFD-DATA-015",
            Self::SingletonKeyMissingOrInvalid => "CFD-DATA-016",
            Self::SingletonKeyCollision => "CFD-DATA-017",
            Self::RefTargetNotFound => "CFD-REF-001",
            Self::CheckFailed => "CFD-CHECK-001",
            Self::CheckEvalTypeError => "CFD-CHECK-002",
            Self::CheckNullAccess => "CFD-CHECK-003",
            Self::CheckIndexOutOfBounds => "CFD-CHECK-004",
            Self::CheckMissingDictKey => "CFD-CHECK-005",
            Self::CheckEmptyMinMax => "CFD-CHECK-006",
            Self::CheckComparisonFailed => "CFD-CHECK-007",
            Self::CheckBoolExpectedTrue => "CFD-CHECK-008",
            Self::CheckNegationFailed => "CFD-CHECK-009",
            Self::CheckAndFailed => "CFD-CHECK-010",
            Self::CheckOrFailed => "CFD-CHECK-011",
            Self::CheckTypePredicateFailed => "CFD-CHECK-012",
            Self::CheckNullPredicateFailed => "CFD-CHECK-013",
            Self::CheckContainsFailed => "CFD-CHECK-014",
            Self::CheckUniqueFailed => "CFD-CHECK-015",
            Self::CheckMatchesFailed => "CFD-CHECK-016",
            Self::CheckAnyQuantifierFailed => "CFD-CHECK-017",
            Self::CheckNoneQuantifierFailed => "CFD-CHECK-018",
            Self::CheckAllQuantifierFailed => "CFD-CHECK-019",
        }
    }

    #[must_use]
    pub fn stage(self) -> CfdStage {
        match self {
            Self::RefTargetNotFound => CfdStage::Reference,
            Self::CheckFailed
            | Self::CheckEvalTypeError
            | Self::CheckNullAccess
            | Self::CheckIndexOutOfBounds
            | Self::CheckMissingDictKey
            | Self::CheckEmptyMinMax
            | Self::CheckComparisonFailed
            | Self::CheckBoolExpectedTrue
            | Self::CheckNegationFailed
            | Self::CheckAndFailed
            | Self::CheckOrFailed
            | Self::CheckTypePredicateFailed
            | Self::CheckNullPredicateFailed
            | Self::CheckContainsFailed
            | Self::CheckUniqueFailed
            | Self::CheckMatchesFailed
            | Self::CheckAnyQuantifierFailed
            | Self::CheckNoneQuantifierFailed
            | Self::CheckAllQuantifierFailed => CfdStage::Check,
            _ => CfdStage::DataModel,
        }
    }
}

impl fmt::Display for CfdErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
