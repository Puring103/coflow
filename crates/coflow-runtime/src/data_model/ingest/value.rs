use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub enum LoadedValueDraft {
    OptionNone,
    OptionSome(Box<LoadedValueDraft>),
    ResultOk(Box<LoadedValueDraft>),
    ResultErr(Box<LoadedValueDraft>),
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
    pub key: Option<String>,
    pub path: Vec<String>,
}

impl LoadedValueDraft {
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
    pub fn record_ref(key: impl Into<String>) -> Self {
        Self::RecordRef(key.into())
    }
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
    EnumVariant { enum_name: String, variant: String },
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
