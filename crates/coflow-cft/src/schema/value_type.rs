use super::{EnumName, TypeName};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CftValueType {
    Int,
    Float,
    Bool,
    String,
    Object(TypeName),
    Enum(EnumName),
    RecordRef(TypeName),
    Array(Box<CftValueType>),
    Dict(Box<CftValueType>, Box<CftValueType>),
    Nullable(Box<CftValueType>),
}

impl CftValueType {
    #[must_use]
    pub const fn is_nullable(&self) -> bool {
        matches!(self, Self::Nullable(_))
    }

    #[must_use]
    pub fn non_nullable(&self) -> &Self {
        match self {
            Self::Nullable(inner) => inner.non_nullable(),
            other => other,
        }
    }

    #[must_use]
    pub fn display_label(&self) -> String {
        self.to_string()
    }
}

impl fmt::Display for CftValueType {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Int => formatter.write_str("int"),
            Self::Float => formatter.write_str("float"),
            Self::Bool => formatter.write_str("bool"),
            Self::String => formatter.write_str("string"),
            Self::Object(name) => name.fmt(formatter),
            Self::Enum(name) => name.fmt(formatter),
            Self::RecordRef(name) => write!(formatter, "&{name}"),
            Self::Array(inner) => write!(formatter, "[{inner}]"),
            Self::Dict(key, value) => write!(formatter, "{{{key}: {value}}}"),
            Self::Nullable(inner) => write!(formatter, "{inner}?"),
        }
    }
}
