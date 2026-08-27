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
    Option(Box<CftValueType>),
    Result(Box<CftValueType>, Box<CftValueType>),
    Function(Vec<CftValueType>, Box<CftValueType>),
    Unit,
}

impl CftValueType {
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
            Self::Option(inner) => write!(formatter, "Option<{inner}>"),
            Self::Result(value, error) => write!(formatter, "Result<{value}, {error}>"),
            Self::Function(parameters, result) => {
                formatter.write_str("fn(")?;
                for (index, parameter) in parameters.iter().enumerate() {
                    if index != 0 {
                        formatter.write_str(", ")?;
                    }
                    parameter.fmt(formatter)?;
                }
                write!(formatter, ") -> {result}")
            }
            Self::Unit => formatter.write_str("()"),
        }
    }
}
