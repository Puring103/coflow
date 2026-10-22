use std::fmt;

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
    InvalidRecordId,
    DuplicateId,
    DuplicatePolymorphicId,
    SingletonRecordCountInvalid,
    SingletonKeyMissingOrInvalid,
    SingletonKeyCollision,
    ValueDependencyCycle,
    DataStructureLimitExceeded,
    RefTargetNotFound,
    RefTargetTypeMismatch,
    CheckFailed,
    CheckEvalTypeError,
    CheckIndexOutOfBounds,
    CheckMissingDictKey,
    CheckEmptyMinMax,
    CheckComparisonFailed,
    CheckBoolExpectedTrue,
    CheckNegationFailed,
    CheckAndFailed,
    CheckOrFailed,
    CheckTypePredicateFailed,
    CheckContainsFailed,
    CheckUniqueFailed,
    CheckMatchesFailed,
    CheckAnyQuantifierFailed,
    CheckNoneQuantifierFailed,
    CheckAllQuantifierFailed,
    CheckBudgetExceeded,
}

// 同一张穷举表生成错误码映射与可遍历目录，新增枚举值必须补齐目录项。
macro_rules! cfd_error_entries {
    ($($variant:ident => ($stage:expr, $code:literal),)*) => {
        impl CfdErrorCode {
            pub const ALL: &'static [Self] = &[$(Self::$variant),*];
            const fn entry(self) -> (CfdStage, &'static str) {
                match self { $(Self::$variant => ($stage, $code),)* }
            }
        }
    };
}
cfd_error_entries! {
    UnknownType => (CfdStage::DataModel, "DATA-001"),
    AbstractRecordType => (CfdStage::DataModel, "DATA-002"),
    MissingObjectType => (CfdStage::DataModel, "DATA-003"),
    ObjectTypeMismatch => (CfdStage::DataModel, "DATA-004"),
    UnknownField => (CfdStage::DataModel, "DATA-005"),
    MissingRequiredField => (CfdStage::DataModel, "DATA-006"),
    TypeMismatch => (CfdStage::DataModel, "DATA-007"),
    InvalidEnumVariant => (CfdStage::DataModel, "DATA-008"),
    DuplicateDictKey => (CfdStage::DataModel, "DATA-009"),
    MissingIdField => (CfdStage::DataModel, "DATA-010"),
    DuplicateId => (CfdStage::DataModel, "DATA-011"),
    DuplicatePolymorphicId => (CfdStage::DataModel, "DATA-012"),
    InvalidRecordId => (CfdStage::DataModel, "DATA-013"),
    ValueDependencyCycle => (CfdStage::DataModel, "DATA-014"),
    SingletonRecordCountInvalid => (CfdStage::DataModel, "DATA-015"),
    SingletonKeyMissingOrInvalid => (CfdStage::DataModel, "DATA-016"),
    SingletonKeyCollision => (CfdStage::DataModel, "DATA-017"),
    DataStructureLimitExceeded => (CfdStage::DataModel, "DATA-018"),
    RefTargetNotFound => (CfdStage::Reference, "REF-001"),
    RefTargetTypeMismatch => (CfdStage::Reference, "REF-002"),
    CheckFailed => (CfdStage::Check, "CHECK-001"),
    CheckEvalTypeError => (CfdStage::Check, "CHECK-002"),
    CheckIndexOutOfBounds => (CfdStage::Check, "CHECK-004"),
    CheckMissingDictKey => (CfdStage::Check, "CHECK-005"),
    CheckEmptyMinMax => (CfdStage::Check, "CHECK-006"),
    CheckComparisonFailed => (CfdStage::Check, "CHECK-007"),
    CheckBoolExpectedTrue => (CfdStage::Check, "CHECK-008"),
    CheckNegationFailed => (CfdStage::Check, "CHECK-009"),
    CheckAndFailed => (CfdStage::Check, "CHECK-010"),
    CheckOrFailed => (CfdStage::Check, "CHECK-011"),
    CheckTypePredicateFailed => (CfdStage::Check, "CHECK-012"),
    CheckContainsFailed => (CfdStage::Check, "CHECK-014"),
    CheckUniqueFailed => (CfdStage::Check, "CHECK-015"),
    CheckMatchesFailed => (CfdStage::Check, "CHECK-016"),
    CheckAnyQuantifierFailed => (CfdStage::Check, "CHECK-017"),
    CheckNoneQuantifierFailed => (CfdStage::Check, "CHECK-018"),
    CheckAllQuantifierFailed => (CfdStage::Check, "CHECK-019"),
    CheckBudgetExceeded => (CfdStage::Check, "CHECK-020"),
}

impl CfdErrorCode {

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        self.entry().1
    }

    #[must_use]
    pub const fn stage(self) -> CfdStage {
        self.entry().0
    }
}

impl fmt::Display for CfdErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
