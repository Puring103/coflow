use crate::schema::{CftConstValue, CftValueType};
use crate::{EnumName, TypeName};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum InferredType {
    Value(CftValueType),
    Null,
    EnumNamespace(EnumName),
    Entry(Box<InferredType>, Box<InferredType>),
    EmptyArray,
    EmptyObject,
    Unknown,
}

impl InferredType {
    pub(super) const fn int() -> Self {
        Self::Value(CftValueType::Int)
    }

    pub(super) const fn float() -> Self {
        Self::Value(CftValueType::Float)
    }

    pub(super) const fn bool() -> Self {
        Self::Value(CftValueType::Bool)
    }

    pub(super) const fn string() -> Self {
        Self::Value(CftValueType::String)
    }

    pub(super) fn object(name: TypeName) -> Self {
        Self::Value(CftValueType::Object(name))
    }

    pub(super) fn enum_value(name: EnumName) -> Self {
        Self::Value(CftValueType::Enum(name))
    }

    pub(super) fn record_ref(target: Self) -> Self {
        match target {
            Self::Value(CftValueType::Object(name)) => Self::Value(CftValueType::RecordRef(name)),
            _ => Self::Unknown,
        }
    }

    pub(super) fn array(element: Self) -> Self {
        match element {
            Self::Value(element) => Self::Value(CftValueType::Array(Box::new(element))),
            _ => Self::Unknown,
        }
    }

    pub(super) fn dict(key: Self, value: Self) -> Self {
        match (key, value) {
            (Self::Value(key), Self::Value(value)) => {
                Self::Value(CftValueType::Dict(Box::new(key), Box::new(value)))
            }
            _ => Self::Unknown,
        }
    }

    pub(super) fn nullable(inner: Self) -> Self {
        match inner {
            Self::Value(CftValueType::Nullable(inner)) => {
                Self::Value(CftValueType::Nullable(inner))
            }
            Self::Value(inner) => Self::Value(CftValueType::Nullable(Box::new(inner))),
            _ => Self::Unknown,
        }
    }

    pub(super) fn from_const(value: &CftConstValue) -> Self {
        match value {
            CftConstValue::Int(_) => Self::int(),
            CftConstValue::Float(_) => Self::float(),
            CftConstValue::Bool(_) => Self::bool(),
            CftConstValue::String(_) => Self::string(),
        }
    }

    pub(super) const fn value_type(&self) -> Option<&CftValueType> {
        match self {
            Self::Value(value_type) => Some(value_type),
            Self::Null
            | Self::EnumNamespace(_)
            | Self::Entry(_, _)
            | Self::EmptyArray
            | Self::EmptyObject
            | Self::Unknown => None,
        }
    }

    pub(super) fn object_name(&self) -> Option<&TypeName> {
        match self.value_type()? {
            CftValueType::Object(name) => Some(name),
            _ => None,
        }
    }

    pub(super) fn enum_name(&self) -> Option<&EnumName> {
        match self.value_type()? {
            CftValueType::Enum(name) => Some(name),
            _ => None,
        }
    }

    pub(super) fn array_element(&self) -> Option<Self> {
        match self.value_type()? {
            CftValueType::Array(element) => Some(Self::Value((**element).clone())),
            _ => None,
        }
    }

    pub(super) fn dict_types(&self) -> Option<(Self, Self)> {
        match self.value_type()? {
            CftValueType::Dict(key, value) => {
                Some((Self::Value((**key).clone()), Self::Value((**value).clone())))
            }
            _ => None,
        }
    }

    pub(super) const fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown)
    }

    pub(super) fn is_nullable(&self) -> bool {
        matches!(self, Self::Value(CftValueType::Nullable(_)))
    }
}

pub(super) fn unwrap_nullable(ty: &InferredType) -> InferredType {
    match ty {
        InferredType::Value(CftValueType::Nullable(inner)) => {
            InferredType::Value((**inner).clone())
        }
        other => other.clone(),
    }
}

pub(super) fn unwrap_reference(ty: &InferredType) -> InferredType {
    match ty {
        InferredType::Value(CftValueType::RecordRef(target)) => {
            InferredType::Value(CftValueType::Object(target.clone()))
        }
        other => other.clone(),
    }
}

pub(super) fn is_valid_dict_key(ty: &InferredType) -> bool {
    matches!(
        ty.value_type(),
        Some(CftValueType::Int | CftValueType::String | CftValueType::Enum(_))
    ) || ty.is_unknown()
}

pub(super) fn types_assignable(expected: &InferredType, actual: &InferredType) -> bool {
    if expected.is_unknown() || actual.is_unknown() {
        return true;
    }
    match (expected, actual) {
        (InferredType::Value(CftValueType::Nullable(_)), InferredType::Null)
        | (InferredType::Value(CftValueType::Array(_)), InferredType::EmptyArray)
        | (
            InferredType::Value(CftValueType::Dict(_, _) | CftValueType::Object(_)),
            InferredType::EmptyObject,
        ) => true,
        (InferredType::Value(CftValueType::Nullable(inner)), other) => {
            types_assignable(&InferredType::Value((**inner).clone()), other)
        }
        (InferredType::Value(expected), InferredType::Value(actual)) => expected == actual,
        _ => expected == actual,
    }
}

pub(super) fn types_comparable(left: &InferredType, right: &InferredType) -> bool {
    if left.is_unknown() || right.is_unknown() {
        return true;
    }
    if matches!((left, right), (InferredType::Null, InferredType::Null)) {
        return true;
    }
    if matches!(
        (left, right),
        (
            InferredType::Null,
            InferredType::Value(CftValueType::Nullable(_))
        ) | (
            InferredType::Value(CftValueType::Nullable(_)),
            InferredType::Null
        )
    ) {
        return true;
    }
    match (unwrap_nullable(left), unwrap_nullable(right)) {
        (InferredType::Value(left), InferredType::Value(right)) => {
            comparable_value_types(&left, &right)
        }
        _ => false,
    }
}

fn comparable_value_types(left: &CftValueType, right: &CftValueType) -> bool {
    match (left, right) {
        (CftValueType::Int, CftValueType::Int)
        | (CftValueType::Float, CftValueType::Float)
        | (CftValueType::Bool, CftValueType::Bool)
        | (CftValueType::String, CftValueType::String) => true,
        (CftValueType::Enum(left), CftValueType::Enum(right)) => left == right,
        (CftValueType::Object(left), CftValueType::Object(right))
        | (CftValueType::RecordRef(left), CftValueType::RecordRef(right)) => left == right,
        _ => false,
    }
}

pub(super) fn ordered_comparable(left: &InferredType, right: &InferredType) -> bool {
    match (unwrap_nullable(left), unwrap_nullable(right)) {
        (InferredType::Unknown, _) | (_, InferredType::Unknown) => true,
        (InferredType::Value(left), InferredType::Value(right)) => match (&left, &right) {
            (CftValueType::Int, CftValueType::Int) | (CftValueType::Float, CftValueType::Float) => {
                true
            }
            (CftValueType::Enum(left), CftValueType::Enum(right)) => left == right,
            _ => false,
        },
        _ => false,
    }
}

pub(super) fn unique_supported(ty: &InferredType) -> bool {
    matches!(
        unwrap_nullable(ty).value_type(),
        Some(CftValueType::Int | CftValueType::Bool | CftValueType::String | CftValueType::Enum(_))
    )
}

pub(super) fn min_max_supported(ty: &InferredType) -> bool {
    matches!(
        unwrap_nullable(ty).value_type(),
        Some(CftValueType::Int | CftValueType::Float | CftValueType::Enum(_))
    )
}

pub(super) fn set_element_supported(ty: &InferredType) -> bool {
    matches!(
        unwrap_nullable(ty).value_type(),
        Some(CftValueType::Int | CftValueType::Bool | CftValueType::String | CftValueType::Enum(_))
    )
}

pub(super) fn sorted_element_supported(ty: &InferredType) -> bool {
    !ty.is_nullable()
        && matches!(
            ty.value_type(),
            Some(
                CftValueType::Int
                    | CftValueType::Bool
                    | CftValueType::String
                    | CftValueType::Enum(_)
            )
        )
}
