use crate::schema::CftConstValue;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum CheckedType {
    Int,
    Float,
    Bool,
    String,
    Null,
    Type(String),
    Ref(Box<CheckedType>),
    Enum(String),
    EnumNamespace(String),
    Array(Box<CheckedType>),
    Dict(Box<CheckedType>, Box<CheckedType>),
    Nullable(Box<CheckedType>),
    Entry(Box<CheckedType>, Box<CheckedType>),
    EmptyArray,
    EmptyObject,
    Unknown,
}

impl CheckedType {
    pub(super) fn from_const(value: &CftConstValue) -> Self {
        match value {
            CftConstValue::Int(_) => Self::Int,
            CftConstValue::Float(_) => Self::Float,
            CftConstValue::Bool(_) => Self::Bool,
            CftConstValue::String(_) => Self::String,
        }
    }
}

pub(super) fn unwrap_nullable(ty: &CheckedType) -> &CheckedType {
    match ty {
        CheckedType::Nullable(inner) => inner,
        other => other,
    }
}

pub(super) fn unwrap_reference(ty: &CheckedType) -> &CheckedType {
    match ty {
        CheckedType::Ref(inner) => inner,
        other => other,
    }
}

pub(super) fn is_valid_dict_key(ty: &CheckedType) -> bool {
    matches!(
        ty,
        CheckedType::Int | CheckedType::String | CheckedType::Enum(_) | CheckedType::Unknown
    )
}

pub(super) fn types_assignable(expected: &CheckedType, actual: &CheckedType) -> bool {
    if matches!(expected, CheckedType::Unknown) || matches!(actual, CheckedType::Unknown) {
        return true;
    }
    match (expected, actual) {
        (CheckedType::Nullable(inner), CheckedType::Null) => {
            !matches!(inner.as_ref(), CheckedType::Unknown)
        }
        (CheckedType::Nullable(inner), other) => types_assignable(inner, other),
        (CheckedType::Ref(left), CheckedType::Ref(right)) => types_assignable(left, right),
        (CheckedType::Array(_), CheckedType::EmptyArray)
        | (CheckedType::Dict(_, _) | CheckedType::Type(_), CheckedType::EmptyObject) => true,
        (CheckedType::Enum(left), CheckedType::Enum(right))
        | (CheckedType::Type(left), CheckedType::Type(right)) => left == right,
        _ => expected == actual,
    }
}

pub(super) fn types_comparable(left: &CheckedType, right: &CheckedType) -> bool {
    if matches!(left, CheckedType::Unknown) || matches!(right, CheckedType::Unknown) {
        return true;
    }
    if matches!((left, right), (CheckedType::Null, CheckedType::Null)) {
        return true;
    }
    if matches!(
        (left, right),
        (CheckedType::Null, CheckedType::Nullable(_))
            | (CheckedType::Nullable(_), CheckedType::Null)
    ) {
        return true;
    }
    match (unwrap_nullable(left), unwrap_nullable(right)) {
        (CheckedType::Unknown, _)
        | (_, CheckedType::Unknown)
        | (CheckedType::Null, CheckedType::Null)
        | (CheckedType::Int, CheckedType::Int)
        | (CheckedType::Float, CheckedType::Float)
        | (CheckedType::Bool, CheckedType::Bool)
        | (CheckedType::String, CheckedType::String) => true,
        (CheckedType::Enum(left), CheckedType::Enum(right))
        | (CheckedType::Type(left), CheckedType::Type(right)) => left == right,
        (CheckedType::Ref(left), CheckedType::Ref(right)) => types_comparable(left, right),
        _ => false,
    }
}

pub(super) fn ordered_comparable(left: &CheckedType, right: &CheckedType) -> bool {
    match (unwrap_nullable(left), unwrap_nullable(right)) {
        (CheckedType::Unknown, _)
        | (_, CheckedType::Unknown)
        | (CheckedType::Int, CheckedType::Int)
        | (CheckedType::Float, CheckedType::Float) => true,
        (CheckedType::Enum(left), CheckedType::Enum(right)) => left == right,
        _ => false,
    }
}

pub(super) fn unique_supported(ty: &CheckedType) -> bool {
    matches!(
        unwrap_nullable(ty),
        CheckedType::Int | CheckedType::Bool | CheckedType::String | CheckedType::Enum(_)
    )
}

pub(super) fn min_max_supported(ty: &CheckedType) -> bool {
    matches!(
        unwrap_nullable(ty),
        CheckedType::Int | CheckedType::Float | CheckedType::Enum(_)
    )
}
