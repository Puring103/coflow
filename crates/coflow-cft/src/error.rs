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
    MultipleIdFieldsInTree,
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
    RefTargetMustBeType,
    EnumVariantOnNonEnum,
    UnknownEnumVariant,
    InvalidConstValue,
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
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UnexpectedCharacter => "CFT-LEX-001",
            Self::InvalidStringEscape => "CFT-LEX-002",
            Self::UnterminatedString => "CFT-LEX-003",
            Self::InvalidIntLiteral => "CFT-LEX-004",
            Self::InvalidFloatLiteral => "CFT-LEX-005",
            Self::UnexpectedToken => "CFT-SYN-001",
            Self::UnexpectedEof => "CFT-SYN-002",
            Self::ExpectedIdentifier => "CFT-SYN-003",
            Self::ExpectedToken => "CFT-SYN-004",
            Self::InvalidTopLevelItem => "CFT-SYN-005",
            Self::InvalidChainComparison => "CFT-SYN-006",
            Self::CheckBlockMustBeLast => "CFT-SYN-007",
            Self::InvalidAnnotationSyntax => "CFT-SYN-008",
            Self::InvalidCheckStatement => "CFT-SYN-009",
            Self::DuplicateCheckBlock => "CFT-SYN-010",
            Self::DuplicateModule => "CFT-SCHEMA-001",
            Self::DuplicateGlobalName => "CFT-SCHEMA-002",
            Self::DuplicateFieldName => "CFT-SCHEMA-003",
            Self::DuplicateEnumVariant => "CFT-SCHEMA-004",
            Self::DuplicateEnumValue => "CFT-SCHEMA-005",
            Self::UnknownNamedType => "CFT-SCHEMA-006",
            Self::ParentMustBeType => "CFT-SCHEMA-007",
            Self::UnknownConst => "CFT-SCHEMA-008",
            Self::InheritanceCycle => "CFT-SCHEMA-009",
            Self::InheritSealedType => "CFT-SCHEMA-010",
            Self::DuplicateInheritedField => "CFT-SCHEMA-011",
            Self::ConflictingTypeModifiers => "CFT-SCHEMA-012",
            Self::MultipleIdFieldsInTree => "CFT-SCHEMA-013",
            Self::InvalidDictKeyType => "CFT-SCHEMA-014",
            Self::InvalidDefaultExpression => "CFT-SCHEMA-015",
            Self::DefaultTypeMismatch => "CFT-SCHEMA-016",
            Self::DefaultReferencesField => "CFT-SCHEMA-017",
            Self::InvalidEnumValueSequence => "CFT-SCHEMA-018",
            Self::InvalidFlagEnumValue => "CFT-SCHEMA-019",
            Self::UnknownAnnotation => "CFT-SCHEMA-020",
            Self::DuplicateAnnotation => "CFT-SCHEMA-021",
            Self::AnnotationWithoutTarget => "CFT-SCHEMA-022",
            Self::InvalidAnnotationTarget => "CFT-SCHEMA-023",
            Self::InvalidAnnotationArgument => "CFT-SCHEMA-024",
            Self::InvalidAnnotatedFieldType => "CFT-SCHEMA-025",
            Self::StructRequiresSealedType => "CFT-SCHEMA-026",
            Self::RefTargetMustBeType => "CFT-SCHEMA-027",
            Self::EnumVariantOnNonEnum => "CFT-SCHEMA-028",
            Self::UnknownEnumVariant => "CFT-SCHEMA-029",
            Self::InvalidConstValue => "CFT-SCHEMA-030",
            Self::UnknownValueName => "CFT-TYPE-001",
            Self::UnknownField => "CFT-TYPE-002",
            Self::TypeUnknownEnumVariant => "CFT-TYPE-003",
            Self::TypeEnumVariantOnNonEnum => "CFT-TYPE-004",
            Self::OperatorTypeMismatch => "CFT-TYPE-005",
            Self::ComparisonTypeMismatch => "CFT-TYPE-006",
            Self::ConditionMustBeBool => "CFT-TYPE-007",
            Self::UnknownFunction => "CFT-TYPE-008",
            Self::FunctionArityMismatch => "CFT-TYPE-009",
            Self::FunctionArgTypeMismatch => "CFT-TYPE-010",
            Self::FieldAccessOnNonObject => "CFT-TYPE-011",
            Self::IndexOnNonIndexable => "CFT-TYPE-012",
            Self::IndexTypeMismatch => "CFT-TYPE-013",
            Self::InvalidIsPredicate => "CFT-TYPE-014",
            Self::QuantifierRequiresCollection => "CFT-TYPE-015",
            Self::UniqueUnsupportedElementType => "CFT-TYPE-016",
            Self::BitwiseRequiresIntOrFlagEnum => "CFT-TYPE-017",
            Self::ShiftRequiresInt => "CFT-TYPE-018",
            Self::RegexPatternMustBeLiteral => "CFT-TYPE-019",
            Self::InvalidRegexPattern => "CFT-TYPE-020",
        }
    }

    #[must_use]
    pub fn stage(self) -> CftStage {
        let code = self.as_str();
        if code.starts_with("CFT-LEX") {
            CftStage::Lex
        } else if code.starts_with("CFT-SYN") {
            CftStage::Syn
        } else if code.starts_with("CFT-TYPE") {
            CftStage::Type
        } else {
            CftStage::Schema
        }
    }
}

impl fmt::Display for CftErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
