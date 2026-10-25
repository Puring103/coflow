use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub enum LoadedValueDraft {
    OptionNone,
    OptionSome(Box<LoadedValueDraft>),
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    FormattedString(LoadedFormattedString),
    EnumVariant {
        enum_name: String,
        variant: String,
    },
    EnumValue {
        enum_name: String,
        value: i64,
    },
    Object {
        actual_type: Option<String>,
        fields: BTreeMap<String, LoadedValueDraft>,
    },
    RecordRef(String),
    /// 显式 &`Type::id` 保留目标类型限定，不能在降低阶段只剩 id。
    TypedRecordRef {
        type_name: String,
        id: String,
    },
    Array(Vec<LoadedValueDraft>),
    Dict(Vec<(LoadedDictKeyDraft, LoadedValueDraft)>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct LoadedFormattedString {
    pub source: String,
    pub segments: Vec<LoadedFormatSegment>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LoadedFormatSegment {
    Text(String),
    Reference(LoadedFieldReference),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedFieldReference {
    pub type_name: Option<String>,
    pub id: Option<String>,
    pub path: Vec<String>,
}

impl LoadedValueDraft {
    /// 输入缓存比较保留浮点符号等可观察状态，不使用语言中的数值等值规则。
    #[must_use]
    pub fn same_value(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Float(left), Self::Float(right)) => left.to_bits() == right.to_bits(),
            (Self::OptionSome(left), Self::OptionSome(right)) => left.same_value(right),
            (Self::Array(left), Self::Array(right)) => {
                left.len() == right.len()
                    && left
                        .iter()
                        .zip(right)
                        .all(|(left, right)| left.same_value(right))
            }
            (Self::Dict(left), Self::Dict(right)) => {
                left.len() == right.len()
                    && left
                        .iter()
                        .zip(right)
                        .all(|((left_key, left), (right_key, right))| {
                            left_key == right_key && left.same_value(right)
                        })
            }
            (
                Self::Object {
                    actual_type: left_type,
                    fields: left,
                },
                Self::Object {
                    actual_type: right_type,
                    fields: right,
                },
            ) => left_type == right_type && same_fields(left, right),
            _ => self == other,
        }
    }

    #[must_use]
    pub fn enum_variant(enum_name: impl Into<String>, variant: impl Into<String>) -> Self {
        Self::EnumVariant {
            enum_name: enum_name.into(),
            variant: variant.into(),
        }
    }

    #[must_use]
    pub fn enum_value(enum_name: impl Into<String>, value: i64) -> Self {
        Self::EnumValue {
            enum_name: enum_name.into(),
            value,
        }
    }

    #[must_use]
    pub fn object(
        actual_type: impl Into<String>,
        fields: impl IntoIterator<Item = (impl Into<String>, LoadedValueDraft)>,
    ) -> Self {
        Self::Object {
            actual_type: Some(actual_type.into()),
            fields: fields
                .into_iter()
                .map(|(name, value)| (name.into(), value))
                .collect(),
        }
    }

    #[must_use]
    pub fn object_with_declared_type(
        fields: impl IntoIterator<Item = (impl Into<String>, LoadedValueDraft)>,
    ) -> Self {
        Self::Object {
            actual_type: None,
            fields: fields
                .into_iter()
                .map(|(name, value)| (name.into(), value))
                .collect(),
        }
    }

    #[must_use]
    pub fn dict(entries: impl IntoIterator<Item = (LoadedDictKeyDraft, LoadedValueDraft)>) -> Self {
        Self::Dict(entries.into_iter().collect())
    }

    #[must_use]
    pub fn record_ref(id: impl Into<String>) -> Self {
        Self::RecordRef(id.into())
    }
}

pub(super) fn same_fields(
    left: &BTreeMap<String, LoadedValueDraft>,
    right: &BTreeMap<String, LoadedValueDraft>,
) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|((left_key, left), (right_key, right))| {
                left_key == right_key && left.same_value(right)
            })
}

impl From<bool> for LoadedValueDraft {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

impl From<i64> for LoadedValueDraft {
    fn from(value: i64) -> Self {
        Self::Int(value)
    }
}

impl From<f64> for LoadedValueDraft {
    fn from(value: f64) -> Self {
        Self::Float(value)
    }
}

impl From<&str> for LoadedValueDraft {
    fn from(value: &str) -> Self {
        Self::String(value.to_string())
    }
}

impl From<String> for LoadedValueDraft {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LoadedDictKeyDraft {
    String(String),
    Int(i64),
    EnumVariant {
        enum_name: String,
        variant: String,
    },
    /// 已解析常量的枚举键保留位模式，组合 flag 不需要虚构成员名。
    EnumValue {
        enum_name: String,
        value: i64,
    },
}

impl LoadedDictKeyDraft {
    #[must_use]
    pub fn enum_variant(enum_name: impl Into<String>, variant: impl Into<String>) -> Self {
        Self::EnumVariant {
            enum_name: enum_name.into(),
            variant: variant.into(),
        }
    }
}

impl From<&str> for LoadedDictKeyDraft {
    fn from(value: &str) -> Self {
        Self::String(value.to_string())
    }
}

impl From<String> for LoadedDictKeyDraft {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl From<i64> for LoadedDictKeyDraft {
    fn from(value: i64) -> Self {
        Self::Int(value)
    }
}
