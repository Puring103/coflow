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
            Self::UnknownType => (CfdStage::DataModel, "CFD-DATA-001"),
            Self::AbstractRecordType => (CfdStage::DataModel, "CFD-DATA-002"),
            Self::MissingObjectType => (CfdStage::DataModel, "CFD-DATA-003"),
            Self::ObjectTypeMismatch => (CfdStage::DataModel, "CFD-DATA-004"),
            Self::UnknownField => (CfdStage::DataModel, "CFD-DATA-005"),
            Self::MissingRequiredField => (CfdStage::DataModel, "CFD-DATA-006"),
            Self::TypeMismatch => (CfdStage::DataModel, "CFD-DATA-007"),
            Self::InvalidEnumVariant => (CfdStage::DataModel, "CFD-DATA-008"),
            Self::DuplicateDictKey => (CfdStage::DataModel, "CFD-DATA-009"),
            Self::MissingIdField => (CfdStage::DataModel, "CFD-DATA-010"),
            Self::DuplicateId => (CfdStage::DataModel, "CFD-DATA-011"),
            Self::DuplicatePolymorphicId => (CfdStage::DataModel, "CFD-DATA-012"),
            Self::InvalidRecordKey => (CfdStage::DataModel, "CFD-DATA-013"),
            Self::ValueDependencyCycle => (CfdStage::DataModel, "CFD-DATA-014"),
            Self::SingletonRecordCountInvalid => (CfdStage::DataModel, "CFD-DATA-015"),
            Self::SingletonKeyMissingOrInvalid => (CfdStage::DataModel, "CFD-DATA-016"),
            Self::SingletonKeyCollision => (CfdStage::DataModel, "CFD-DATA-017"),
            Self::DataStructureLimitExceeded => (CfdStage::DataModel, "CFD-DATA-018"),
            Self::RefTargetNotFound => (CfdStage::Reference, "CFD-REF-001"),
            Self::CheckFailed => (CfdStage::Check, "CFD-CHECK-001"),
            Self::CheckEvalTypeError => (CfdStage::Check, "CFD-CHECK-002"),
            Self::CheckNullAccess => (CfdStage::Check, "CFD-CHECK-003"),
            Self::CheckIndexOutOfBounds => (CfdStage::Check, "CFD-CHECK-004"),
            Self::CheckMissingDictKey => (CfdStage::Check, "CFD-CHECK-005"),
            Self::CheckEmptyMinMax => (CfdStage::Check, "CFD-CHECK-006"),
            Self::CheckComparisonFailed => (CfdStage::Check, "CFD-CHECK-007"),
            Self::CheckBoolExpectedTrue => (CfdStage::Check, "CFD-CHECK-008"),
            Self::CheckNegationFailed => (CfdStage::Check, "CFD-CHECK-009"),
            Self::CheckAndFailed => (CfdStage::Check, "CFD-CHECK-010"),
            Self::CheckOrFailed => (CfdStage::Check, "CFD-CHECK-011"),
            Self::CheckTypePredicateFailed => (CfdStage::Check, "CFD-CHECK-012"),
            Self::CheckNullPredicateFailed => (CfdStage::Check, "CFD-CHECK-013"),
            Self::CheckContainsFailed => (CfdStage::Check, "CFD-CHECK-014"),
            Self::CheckUniqueFailed => (CfdStage::Check, "CFD-CHECK-015"),
            Self::CheckMatchesFailed => (CfdStage::Check, "CFD-CHECK-016"),
            Self::CheckAnyQuantifierFailed => (CfdStage::Check, "CFD-CHECK-017"),
            Self::CheckNoneQuantifierFailed => (CfdStage::Check, "CFD-CHECK-018"),
            Self::CheckAllQuantifierFailed => (CfdStage::Check, "CFD-CHECK-019"),
            Self::CheckBudgetExceeded => (CfdStage::Check, "CFD-CHECK-020"),
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
