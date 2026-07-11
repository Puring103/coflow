use crate::container::ModuleId;
use crate::span::Span;
use std::fmt;

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
pub enum CftStage {
    Lex,
    Syn,
    Schema,
    Type,
}

impl fmt::Display for CftStage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Lex => "LEX",
            Self::Syn => "SYN",
            Self::Schema => "SCHEMA",
            Self::Type => "TYPE",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CftSeverity {
    Error,
}

/// Stable error codes for CFT diagnostics.
///
/// Each variant maps to a string code (`CFT-LEX-001`, `CFT-SCHEMA-029`, …) via
/// [`CftErrorCode::as_str`]. The string codes are the stable identifiers and
/// must not change; the Rust variant names are an internal convenience.
///
/// **Why some `TYPE-*` variants carry a `Type` prefix in Rust:** the spec
/// reuses the same human names (`UnknownEnumVariant`, `EnumVariantOnNonEnum`)
/// for both `SCHEMA-*` and `TYPE-*` codes. Rust enums can't have two variants
/// with the same name, so the `TYPE-*` ones are renamed
/// [`Self::TypeUnknownEnumVariant`] / [`Self::TypeEnumVariantOnNonEnum`].
/// Their `as_str()` codes (`CFT-TYPE-003` / `CFT-TYPE-004`) are unaffected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(clippy::enum_variant_names)]
pub enum CftErrorCode {
    UnexpectedCharacter,
    InvalidStringEscape,
    UnterminatedString,
    InvalidIntLiteral,
    InvalidFloatLiteral,
    UnexpectedToken,
    UnexpectedEof,
    ExpectedIdentifier,
    ExpectedToken,
    InvalidTopLevelItem,
    InvalidChainComparison,
    CheckBlockMustBeLast,
    InvalidAnnotationSyntax,
    InvalidCheckStatement,
    DuplicateCheckBlock,
    SyntaxStructureLimitExceeded,
    DuplicateModule,
    DuplicateGlobalName,
    DuplicateFieldName,
    DuplicateEnumVariant,
    DuplicateEnumValue,
    UnknownNamedType,
    ParentMustBeType,
    UnknownConst,
    InheritanceCycle,
    InheritSealedType,
    DuplicateInheritedField,
    ConflictingTypeModifiers,
    InvalidDictKeyType,
    InvalidDefaultExpression,
    DefaultTypeMismatch,
    DefaultReferencesField,
    InvalidEnumValueSequence,
    InvalidFlagEnumValue,
    UnknownAnnotation,
    DuplicateAnnotation,
    AnnotationWithoutTarget,
    InvalidAnnotationTarget,
    InvalidAnnotationArgument,
    InvalidAnnotatedFieldType,
    StructRequiresSealedType,
    IdAsEnumRequiresEmptyEnum,
    EnumVariantOnNonEnum,
    UnknownEnumVariant,
    InvalidConstValue,
    ReservedIdentifier,
    LocalizedOnInvalidTarget,
    LocalizedBucketNotIdentifier,
    DimensionOnInvalidTarget,
    DimensionNameNotIdentifier,
    SingletonOnAbstractType,
    SingletonIdAsEnumConflict,
    SchemaStructureLimitExceeded,
    UnknownValueName,
    UnknownField,
    TypeUnknownEnumVariant,
    TypeEnumVariantOnNonEnum,
    OperatorTypeMismatch,
    ComparisonTypeMismatch,
    ConditionMustBeBool,
    UnknownFunction,
    FunctionArityMismatch,
    FunctionArgTypeMismatch,
    FieldAccessOnNonObject,
    IndexOnNonIndexable,
    IndexTypeMismatch,
    InvalidIsPredicate,
    QuantifierRequiresCollection,
    UniqueUnsupportedElementType,
    BitwiseRequiresIntOrFlagEnum,
    ShiftRequiresInt,
    RegexPatternMustBeLiteral,
    InvalidRegexPattern,
}

impl CftErrorCode {
    /// Single source of truth for `(stage, string code)` per error variant.
    /// Both [`Self::as_str`] and [`Self::stage`] read this table — adding a
    /// variant in one place can never silently desync the other.
    const fn entry(self) -> (CftStage, &'static str) {
        match self {
            Self::UnexpectedCharacter => (CftStage::Lex, "CFT-LEX-001"),
            Self::InvalidStringEscape => (CftStage::Lex, "CFT-LEX-002"),
            Self::UnterminatedString => (CftStage::Lex, "CFT-LEX-003"),
            Self::InvalidIntLiteral => (CftStage::Lex, "CFT-LEX-004"),
            Self::InvalidFloatLiteral => (CftStage::Lex, "CFT-LEX-005"),
            Self::UnexpectedToken => (CftStage::Syn, "CFT-SYN-001"),
            Self::UnexpectedEof => (CftStage::Syn, "CFT-SYN-002"),
            Self::ExpectedIdentifier => (CftStage::Syn, "CFT-SYN-003"),
            Self::ExpectedToken => (CftStage::Syn, "CFT-SYN-004"),
            Self::InvalidTopLevelItem => (CftStage::Syn, "CFT-SYN-005"),
            Self::InvalidChainComparison => (CftStage::Syn, "CFT-SYN-006"),
            Self::CheckBlockMustBeLast => (CftStage::Syn, "CFT-SYN-007"),
            Self::InvalidAnnotationSyntax => (CftStage::Syn, "CFT-SYN-008"),
            Self::InvalidCheckStatement => (CftStage::Syn, "CFT-SYN-009"),
            Self::DuplicateCheckBlock => (CftStage::Syn, "CFT-SYN-010"),
            Self::SyntaxStructureLimitExceeded => (CftStage::Syn, "CFT-SYN-011"),
            Self::DuplicateModule => (CftStage::Schema, "CFT-SCHEMA-001"),
            Self::DuplicateGlobalName => (CftStage::Schema, "CFT-SCHEMA-002"),
            Self::DuplicateFieldName => (CftStage::Schema, "CFT-SCHEMA-003"),
            Self::DuplicateEnumVariant => (CftStage::Schema, "CFT-SCHEMA-004"),
            Self::DuplicateEnumValue => (CftStage::Schema, "CFT-SCHEMA-005"),
            Self::UnknownNamedType => (CftStage::Schema, "CFT-SCHEMA-006"),
            Self::ParentMustBeType => (CftStage::Schema, "CFT-SCHEMA-007"),
            Self::UnknownConst => (CftStage::Schema, "CFT-SCHEMA-008"),
            Self::InheritanceCycle => (CftStage::Schema, "CFT-SCHEMA-009"),
            Self::InheritSealedType => (CftStage::Schema, "CFT-SCHEMA-010"),
            Self::DuplicateInheritedField => (CftStage::Schema, "CFT-SCHEMA-011"),
            Self::ConflictingTypeModifiers => (CftStage::Schema, "CFT-SCHEMA-012"),
            Self::InvalidDictKeyType => (CftStage::Schema, "CFT-SCHEMA-014"),
            Self::InvalidDefaultExpression => (CftStage::Schema, "CFT-SCHEMA-015"),
            Self::DefaultTypeMismatch => (CftStage::Schema, "CFT-SCHEMA-016"),
            Self::DefaultReferencesField => (CftStage::Schema, "CFT-SCHEMA-017"),
            Self::InvalidEnumValueSequence => (CftStage::Schema, "CFT-SCHEMA-018"),
            Self::InvalidFlagEnumValue => (CftStage::Schema, "CFT-SCHEMA-019"),
            Self::UnknownAnnotation => (CftStage::Schema, "CFT-SCHEMA-020"),
            Self::DuplicateAnnotation => (CftStage::Schema, "CFT-SCHEMA-021"),
            Self::AnnotationWithoutTarget => (CftStage::Schema, "CFT-SCHEMA-022"),
            Self::InvalidAnnotationTarget => (CftStage::Schema, "CFT-SCHEMA-023"),
            Self::InvalidAnnotationArgument => (CftStage::Schema, "CFT-SCHEMA-024"),
            Self::InvalidAnnotatedFieldType => (CftStage::Schema, "CFT-SCHEMA-025"),
            Self::StructRequiresSealedType => (CftStage::Schema, "CFT-SCHEMA-026"),
            Self::IdAsEnumRequiresEmptyEnum => (CftStage::Schema, "CFT-SCHEMA-027"),
            Self::EnumVariantOnNonEnum => (CftStage::Schema, "CFT-SCHEMA-028"),
            Self::UnknownEnumVariant => (CftStage::Schema, "CFT-SCHEMA-029"),
            Self::InvalidConstValue => (CftStage::Schema, "CFT-SCHEMA-030"),
            Self::ReservedIdentifier => (CftStage::Schema, "CFT-SCHEMA-031"),
            Self::LocalizedOnInvalidTarget => (CftStage::Schema, "CFT-SCHEMA-034"),
            Self::LocalizedBucketNotIdentifier => (CftStage::Schema, "CFT-SCHEMA-035"),
            Self::DimensionOnInvalidTarget => (CftStage::Schema, "CFT-SCHEMA-032"),
            Self::DimensionNameNotIdentifier => (CftStage::Schema, "CFT-SCHEMA-033"),
            Self::SingletonOnAbstractType => (CftStage::Schema, "CFT-SCHEMA-036"),
            Self::SingletonIdAsEnumConflict => (CftStage::Schema, "CFT-SCHEMA-037"),
            Self::SchemaStructureLimitExceeded => (CftStage::Schema, "CFT-SCHEMA-038"),
            Self::UnknownValueName => (CftStage::Type, "CFT-TYPE-001"),
            Self::UnknownField => (CftStage::Type, "CFT-TYPE-002"),
            Self::TypeUnknownEnumVariant => (CftStage::Type, "CFT-TYPE-003"),
            Self::TypeEnumVariantOnNonEnum => (CftStage::Type, "CFT-TYPE-004"),
            Self::OperatorTypeMismatch => (CftStage::Type, "CFT-TYPE-005"),
            Self::ComparisonTypeMismatch => (CftStage::Type, "CFT-TYPE-006"),
            Self::ConditionMustBeBool => (CftStage::Type, "CFT-TYPE-007"),
            Self::UnknownFunction => (CftStage::Type, "CFT-TYPE-008"),
            Self::FunctionArityMismatch => (CftStage::Type, "CFT-TYPE-009"),
            Self::FunctionArgTypeMismatch => (CftStage::Type, "CFT-TYPE-010"),
            Self::FieldAccessOnNonObject => (CftStage::Type, "CFT-TYPE-011"),
            Self::IndexOnNonIndexable => (CftStage::Type, "CFT-TYPE-012"),
            Self::IndexTypeMismatch => (CftStage::Type, "CFT-TYPE-013"),
            Self::InvalidIsPredicate => (CftStage::Type, "CFT-TYPE-014"),
            Self::QuantifierRequiresCollection => (CftStage::Type, "CFT-TYPE-015"),
            Self::UniqueUnsupportedElementType => (CftStage::Type, "CFT-TYPE-016"),
            Self::BitwiseRequiresIntOrFlagEnum => (CftStage::Type, "CFT-TYPE-017"),
            Self::ShiftRequiresInt => (CftStage::Type, "CFT-TYPE-018"),
            Self::RegexPatternMustBeLiteral => (CftStage::Type, "CFT-TYPE-019"),
            Self::InvalidRegexPattern => (CftStage::Type, "CFT-TYPE-020"),
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        self.entry().1
    }

    #[must_use]
    pub const fn stage(self) -> CftStage {
        self.entry().0
    }
}

impl fmt::Display for CftErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
