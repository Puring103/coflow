use crate::schema::{CftFunctionParameter, CftValueType};
use crate::{EnumName, TypeName};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum InferredType {
    Value(CftValueType),
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

    pub(super) fn option(inner: Self) -> Self {
        match inner {
            Self::Value(inner) => Self::Value(CftValueType::Option(Box::new(inner))),
            _ => Self::Unknown,
        }
    }

    pub(super) fn function(parameters: Vec<(Option<String>, Self)>, result: Self) -> Self {
        let Some(parameters) = parameters
            .into_iter()
            .map(|(name, parameter)| {
                parameter
                    .value_type()
                    .cloned()
                    .map(|value_type| CftFunctionParameter { name, value_type })
            })
            .collect::<Option<Vec<_>>>()
        else {
            return Self::Unknown;
        };
        match result {
            Self::Value(result) => {
                Self::Value(CftValueType::Function(parameters, Box::new(result)))
            }
            _ => Self::Unknown,
        }
    }

    pub(super) const fn unit() -> Self {
        Self::Value(CftValueType::Unit)
    }

    pub(super) const fn value_type(&self) -> Option<&CftValueType> {
        match self {
            Self::Value(value_type) => Some(value_type),
            Self::Unknown => None,
        }
    }

    pub(super) const fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown)
    }
}

pub(super) fn is_valid_dict_key(ty: &InferredType) -> bool {
    matches!(
        ty.value_type(),
        Some(CftValueType::Int | CftValueType::Bool | CftValueType::String | CftValueType::Enum(_))
    ) || ty.is_unknown()
}
