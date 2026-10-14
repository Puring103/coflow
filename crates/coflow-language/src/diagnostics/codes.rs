use std::fmt;

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
    DuplicateTopLevelCheck,
    TypeAliasCycle,
    RequiredObjectCycle,
    DefaultMaterializationCycle,
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
    InvalidQuantifierBindings,
    InvalidRecordSetQuery,
}

// 同一张穷举表生成错误码映射与可遍历目录，新增枚举值必须补齐目录项。
macro_rules! cft_error_entries {
    ($($variant:ident => ($stage:expr, $code:literal),)*) => {
        impl CftErrorCode {
            pub const ALL: &'static [Self] = &[$(Self::$variant),*];
            const fn entry(self) -> (CftStage, &'static str) {
                match self { $(Self::$variant => ($stage, $code),)* }
            }
        }
    };
}
cft_error_entries! {
    UnexpectedCharacter => (CftStage::Lex, "CFT-LEX-001"),
    InvalidStringEscape => (CftStage::Lex, "CFT-LEX-002"),
    UnterminatedString => (CftStage::Lex, "CFT-LEX-003"),
    InvalidIntLiteral => (CftStage::Lex, "CFT-LEX-004"),
    InvalidFloatLiteral => (CftStage::Lex, "CFT-LEX-005"),
    UnexpectedToken => (CftStage::Syn, "CFT-SYN-001"),
    UnexpectedEof => (CftStage::Syn, "CFT-SYN-002"),
    ExpectedIdentifier => (CftStage::Syn, "CFT-SYN-003"),
    ExpectedToken => (CftStage::Syn, "CFT-SYN-004"),
    InvalidTopLevelItem => (CftStage::Syn, "CFT-SYN-005"),
    InvalidChainComparison => (CftStage::Syn, "CFT-SYN-006"),
    CheckBlockMustBeLast => (CftStage::Syn, "CFT-SYN-007"),
    InvalidAnnotationSyntax => (CftStage::Syn, "CFT-SYN-008"),
    InvalidCheckStatement => (CftStage::Syn, "CFT-SYN-009"),
    DuplicateCheckBlock => (CftStage::Syn, "CFT-SYN-010"),
    SyntaxStructureLimitExceeded => (CftStage::Syn, "CFT-SYN-011"),
    DuplicateModule => (CftStage::Schema, "CFT-SCHEMA-001"),
    DuplicateGlobalName => (CftStage::Schema, "CFT-SCHEMA-002"),
    DuplicateFieldName => (CftStage::Schema, "CFT-SCHEMA-003"),
    DuplicateEnumVariant => (CftStage::Schema, "CFT-SCHEMA-004"),
    DuplicateEnumValue => (CftStage::Schema, "CFT-SCHEMA-005"),
    UnknownNamedType => (CftStage::Schema, "CFT-SCHEMA-006"),
    ParentMustBeType => (CftStage::Schema, "CFT-SCHEMA-007"),
    UnknownConst => (CftStage::Schema, "CFT-SCHEMA-008"),
    InheritanceCycle => (CftStage::Schema, "CFT-SCHEMA-009"),
    InheritSealedType => (CftStage::Schema, "CFT-SCHEMA-010"),
    DuplicateInheritedField => (CftStage::Schema, "CFT-SCHEMA-011"),
    ConflictingTypeModifiers => (CftStage::Schema, "CFT-SCHEMA-012"),
    InvalidDictKeyType => (CftStage::Schema, "CFT-SCHEMA-014"),
    InvalidDefaultExpression => (CftStage::Schema, "CFT-SCHEMA-015"),
    DefaultTypeMismatch => (CftStage::Schema, "CFT-SCHEMA-016"),
    DefaultReferencesField => (CftStage::Schema, "CFT-SCHEMA-017"),
    InvalidEnumValueSequence => (CftStage::Schema, "CFT-SCHEMA-018"),
    InvalidFlagEnumValue => (CftStage::Schema, "CFT-SCHEMA-019"),
    UnknownAnnotation => (CftStage::Schema, "CFT-SCHEMA-020"),
    DuplicateAnnotation => (CftStage::Schema, "CFT-SCHEMA-021"),
    AnnotationWithoutTarget => (CftStage::Schema, "CFT-SCHEMA-022"),
    InvalidAnnotationTarget => (CftStage::Schema, "CFT-SCHEMA-023"),
    InvalidAnnotationArgument => (CftStage::Schema, "CFT-SCHEMA-024"),
    InvalidAnnotatedFieldType => (CftStage::Schema, "CFT-SCHEMA-025"),
    StructRequiresSealedType => (CftStage::Schema, "CFT-SCHEMA-026"),
    IdAsEnumRequiresEmptyEnum => (CftStage::Schema, "CFT-SCHEMA-027"),
    EnumVariantOnNonEnum => (CftStage::Schema, "CFT-SCHEMA-028"),
    UnknownEnumVariant => (CftStage::Schema, "CFT-SCHEMA-029"),
    InvalidConstValue => (CftStage::Schema, "CFT-SCHEMA-030"),
    ReservedIdentifier => (CftStage::Schema, "CFT-SCHEMA-031"),
    LocalizedOnInvalidTarget => (CftStage::Schema, "CFT-SCHEMA-034"),
    LocalizedBucketNotIdentifier => (CftStage::Schema, "CFT-SCHEMA-035"),
    DimensionOnInvalidTarget => (CftStage::Schema, "CFT-SCHEMA-032"),
    DimensionNameNotIdentifier => (CftStage::Schema, "CFT-SCHEMA-033"),
    SingletonOnAbstractType => (CftStage::Schema, "CFT-SCHEMA-036"),
    SingletonIdAsEnumConflict => (CftStage::Schema, "CFT-SCHEMA-037"),
    SchemaStructureLimitExceeded => (CftStage::Schema, "CFT-SCHEMA-038"),
    DuplicateTopLevelCheck => (CftStage::Schema, "CFT-SCHEMA-039"),
    TypeAliasCycle => (CftStage::Schema, "CFT-SCHEMA-040"),
    RequiredObjectCycle => (CftStage::Schema, "CFT-SCHEMA-042"),
    DefaultMaterializationCycle => (CftStage::Schema, "CFT-SCHEMA-043"),
    UnknownValueName => (CftStage::Type, "CFT-TYPE-001"),
    UnknownField => (CftStage::Type, "CFT-TYPE-002"),
    TypeUnknownEnumVariant => (CftStage::Type, "CFT-TYPE-003"),
    TypeEnumVariantOnNonEnum => (CftStage::Type, "CFT-TYPE-004"),
    OperatorTypeMismatch => (CftStage::Type, "CFT-TYPE-005"),
    ComparisonTypeMismatch => (CftStage::Type, "CFT-TYPE-006"),
    ConditionMustBeBool => (CftStage::Type, "CFT-TYPE-007"),
    UnknownFunction => (CftStage::Type, "CFT-TYPE-008"),
    FunctionArityMismatch => (CftStage::Type, "CFT-TYPE-009"),
    FunctionArgTypeMismatch => (CftStage::Type, "CFT-TYPE-010"),
    FieldAccessOnNonObject => (CftStage::Type, "CFT-TYPE-011"),
    IndexOnNonIndexable => (CftStage::Type, "CFT-TYPE-012"),
    IndexTypeMismatch => (CftStage::Type, "CFT-TYPE-013"),
    InvalidIsPredicate => (CftStage::Type, "CFT-TYPE-014"),
    QuantifierRequiresCollection => (CftStage::Type, "CFT-TYPE-015"),
    UniqueUnsupportedElementType => (CftStage::Type, "CFT-TYPE-016"),
    BitwiseRequiresIntOrFlagEnum => (CftStage::Type, "CFT-TYPE-017"),
    ShiftRequiresInt => (CftStage::Type, "CFT-TYPE-018"),
    RegexPatternMustBeLiteral => (CftStage::Type, "CFT-TYPE-019"),
    InvalidRegexPattern => (CftStage::Type, "CFT-TYPE-020"),
    InvalidQuantifierBindings => (CftStage::Type, "CFT-TYPE-021"),
    InvalidRecordSetQuery => (CftStage::Type, "CFT-TYPE-022"),
}

impl CftErrorCode {

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
