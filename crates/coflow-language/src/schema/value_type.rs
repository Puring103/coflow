use super::{EnumName, TypeName};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CftValueType {
    Int,
    Long,
    Float,
    Double,
    Bool,
    String,
    Object(TypeName),
    Enum(EnumName),
    RecordRef(TypeName),
    Array(Box<CftValueType>),
    Dict(Box<CftValueType>, Box<CftValueType>),
    Option(Box<CftValueType>),
}

impl CftValueType {
    /// 数值类型按固定顺序提升，禁止通过数值大小隐式缩窄已声明的类型。
    #[must_use]
    pub const fn numeric_rank(&self) -> Option<u8> {
        match self {
            Self::Int => Some(0),
            Self::Long => Some(1),
            Self::Float => Some(2),
            Self::Double => Some(3),
            _ => None,
        }
    }

    #[must_use]
    pub fn promote_numeric(&self, other: &Self) -> Option<Self> {
        Some(if self.numeric_rank()? >= other.numeric_rank()? {
            self.clone()
        } else {
            other.clone()
        })
    }

    #[must_use]
    pub const fn is_integer(&self) -> bool {
        matches!(self, Self::Int | Self::Long)
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
            Self::Long => formatter.write_str("long"),
            Self::Double => formatter.write_str("double"),
            Self::Float => formatter.write_str("float"),
            Self::Bool => formatter.write_str("bool"),
            Self::String => formatter.write_str("string"),
            Self::Object(name) => name.fmt(formatter),
            Self::Enum(name) => name.fmt(formatter),
            Self::RecordRef(name) => write!(formatter, "&{name}"),
            Self::Array(inner) => write!(formatter, "[{inner}]"),
            Self::Dict(key, value) => write!(formatter, "{{{key}: {value}}}"),
            Self::Option(inner) => write!(formatter, "Option<{inner}>"),
        }
    }
}
