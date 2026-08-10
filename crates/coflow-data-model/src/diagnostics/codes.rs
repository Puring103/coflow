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
    InvalidRecordKey,
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
    CheckBudgetExceeded,
}

impl CfdErrorCode {
    #[must_use]
    const fn entry(self) -> (CfdStage, &'static str) {
        match self {
            Self::UnknownType => (CfdStage::DataModel, "DATA-001"),
            Self::AbstractRecordType => (CfdStage::DataModel, "DATA-002"),
            Self::MissingObjectType => (CfdStage::DataModel, "DATA-003"),
            Self::ObjectTypeMismatch => (CfdStage::DataModel, "DATA-004"),
            Self::UnknownField => (CfdStage::DataModel, "DATA-005"),
            Self::MissingRequiredField => (CfdStage::DataModel, "DATA-006"),
            Self::TypeMismatch => (CfdStage::DataModel, "DATA-007"),
            Self::InvalidEnumVariant => (CfdStage::DataModel, "DATA-008"),
            Self::DuplicateDictKey => (CfdStage::DataModel, "DATA-009"),
            Self::MissingIdField => (CfdStage::DataModel, "DATA-010"),
            Self::DuplicateId => (CfdStage::DataModel, "DATA-011"),
            Self::DuplicatePolymorphicId => (CfdStage::DataModel, "DATA-012"),
            Self::InvalidRecordKey => (CfdStage::DataModel, "DATA-013"),
            Self::ValueDependencyCycle => (CfdStage::DataModel, "DATA-014"),
            Self::SingletonRecordCountInvalid => (CfdStage::DataModel, "DATA-015"),
            Self::SingletonKeyMissingOrInvalid => (CfdStage::DataModel, "DATA-016"),
            Self::SingletonKeyCollision => (CfdStage::DataModel, "DATA-017"),
            Self::DataStructureLimitExceeded => (CfdStage::DataModel, "DATA-018"),
            Self::RefTargetNotFound => (CfdStage::Reference, "REF-001"),
            Self::RefTargetTypeMismatch => (CfdStage::Reference, "REF-002"),
            Self::CheckFailed => (CfdStage::Check, "CHECK-001"),
            Self::CheckEvalTypeError => (CfdStage::Check, "CHECK-002"),
            Self::CheckNullAccess => (CfdStage::Check, "CHECK-003"),
            Self::CheckIndexOutOfBounds => (CfdStage::Check, "CHECK-004"),
            Self::CheckMissingDictKey => (CfdStage::Check, "CHECK-005"),
            Self::CheckEmptyMinMax => (CfdStage::Check, "CHECK-006"),
            Self::CheckComparisonFailed => (CfdStage::Check, "CHECK-007"),
            Self::CheckBoolExpectedTrue => (CfdStage::Check, "CHECK-008"),
            Self::CheckNegationFailed => (CfdStage::Check, "CHECK-009"),
            Self::CheckAndFailed => (CfdStage::Check, "CHECK-010"),
            Self::CheckOrFailed => (CfdStage::Check, "CHECK-011"),
            Self::CheckTypePredicateFailed => (CfdStage::Check, "CHECK-012"),
            Self::CheckNullPredicateFailed => (CfdStage::Check, "CHECK-013"),
            Self::CheckContainsFailed => (CfdStage::Check, "CHECK-014"),
            Self::CheckUniqueFailed => (CfdStage::Check, "CHECK-015"),
            Self::CheckMatchesFailed => (CfdStage::Check, "CHECK-016"),
            Self::CheckAnyQuantifierFailed => (CfdStage::Check, "CHECK-017"),
            Self::CheckNoneQuantifierFailed => (CfdStage::Check, "CHECK-018"),
            Self::CheckAllQuantifierFailed => (CfdStage::Check, "CHECK-019"),
            Self::CheckBudgetExceeded => (CfdStage::Check, "CHECK-020"),
        }
    }

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
