use crate::origin::RecordOrigin;
use coflow_cft::{DimensionName, FieldName, RecordKey, TypeName, VariantName};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub struct CfdInputDimensionValue {
    pub source_type: TypeName,
    pub source_key: RecordKey,
    pub field: FieldName,
    pub dimension: DimensionName,
    pub variant: VariantName,
    pub value: CfdInputValue,
    pub origin: RecordOrigin,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CfdInputRecord {
    pub key: String,
    pub actual_type: String,
    pub spreads: Vec<CfdInputValue>,
    pub fields: BTreeMap<String, CfdInputValue>,
    /// Where this top-level record originated. Loaders set this when parsing;
    /// synthetic records (tests, ad-hoc construction) leave it as
    /// [`RecordOrigin::None`]. The compiler moves this onto the resulting
    /// [`CfdRecord`].
    pub origin: RecordOrigin,
}

impl CfdInputRecord {
    #[must_use]
    pub fn new(
        key: impl Into<String>,
        actual_type: impl Into<String>,
        fields: impl IntoIterator<Item = (impl Into<String>, CfdInputValue)>,
    ) -> Self {
        Self {
            key: key.into(),
            actual_type: actual_type.into(),
            spreads: Vec::new(),
            fields: fields
                .into_iter()
                .map(|(name, value)| (name.into(), value))
                .collect(),
            origin: RecordOrigin::None,
        }
    }

    #[must_use]
    pub fn with_spreads(
        key: impl Into<String>,
        actual_type: impl Into<String>,
        spreads: impl IntoIterator<Item = CfdInputValue>,
        fields: impl IntoIterator<Item = (impl Into<String>, CfdInputValue)>,
    ) -> Self {
        Self {
            key: key.into(),
            actual_type: actual_type.into(),
            spreads: spreads.into_iter().collect(),
            fields: fields
                .into_iter()
                .map(|(name, value)| (name.into(), value))
                .collect(),
            origin: RecordOrigin::None,
        }
    }

    /// Builder-style: attach an origin to this input record.
    #[must_use]
    pub fn with_origin(mut self, origin: RecordOrigin) -> Self {
        self.origin = origin;
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CfdInputValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    EnumVariant {
        enum_name: String,
        variant: String,
    },
    Object {
        actual_type: Option<String>,
        fields: BTreeMap<String, CfdInputValue>,
    },
    ObjectSpread {
        actual_type: Option<String>,
        spreads: Vec<CfdInputValue>,
        fields: BTreeMap<String, CfdInputValue>,
    },
    RecordRef(String),
    Array(Vec<CfdInputValue>),
    Dict(Vec<(CfdInputDictKey, CfdInputValue)>),
    DictSpread {
        spreads: Vec<CfdInputValue>,
        entries: Vec<(CfdInputDictKey, CfdInputValue)>,
    },
}

impl CfdInputValue {
    #[must_use]
    pub fn enum_variant(enum_name: impl Into<String>, variant: impl Into<String>) -> Self {
        Self::EnumVariant {
            enum_name: enum_name.into(),
            variant: variant.into(),
        }
    }

    #[must_use]
    pub fn object(
        actual_type: impl Into<String>,
        fields: impl IntoIterator<Item = (impl Into<String>, CfdInputValue)>,
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
        fields: impl IntoIterator<Item = (impl Into<String>, CfdInputValue)>,
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
    pub fn object_spread(
        spreads: impl IntoIterator<Item = CfdInputValue>,
        fields: impl IntoIterator<Item = (impl Into<String>, CfdInputValue)>,
    ) -> Self {
        Self::ObjectSpread {
            actual_type: None,
            spreads: spreads.into_iter().collect(),
            fields: fields
                .into_iter()
                .map(|(name, value)| (name.into(), value))
                .collect(),
        }
    }

    #[must_use]
    pub fn object_spread_with_actual_type(
        actual_type: impl Into<String>,
        spreads: impl IntoIterator<Item = CfdInputValue>,
        fields: impl IntoIterator<Item = (impl Into<String>, CfdInputValue)>,
    ) -> Self {
        Self::ObjectSpread {
            actual_type: Some(actual_type.into()),
            spreads: spreads.into_iter().collect(),
            fields: fields
                .into_iter()
                .map(|(name, value)| (name.into(), value))
                .collect(),
        }
    }

    #[must_use]
    pub fn dict(entries: impl IntoIterator<Item = (CfdInputDictKey, CfdInputValue)>) -> Self {
        Self::Dict(entries.into_iter().collect())
    }

    #[must_use]
    pub fn dict_spread(
        spreads: impl IntoIterator<Item = CfdInputValue>,
        entries: impl IntoIterator<Item = (CfdInputDictKey, CfdInputValue)>,
    ) -> Self {
        Self::DictSpread {
            spreads: spreads.into_iter().collect(),
            entries: entries.into_iter().collect(),
        }
    }

    #[must_use]
    pub fn record_ref(key: impl Into<String>) -> Self {
        Self::RecordRef(key.into())
    }
}

impl From<bool> for CfdInputValue {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

impl From<i64> for CfdInputValue {
    fn from(value: i64) -> Self {
        Self::Int(value)
    }
}

impl From<f64> for CfdInputValue {
    fn from(value: f64) -> Self {
        Self::Float(value)
    }
}

impl From<&str> for CfdInputValue {
    fn from(value: &str) -> Self {
        Self::String(value.to_string())
    }
}

impl From<String> for CfdInputValue {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CfdInputDictKey {
    String(String),
    Int(i64),
    EnumVariant { enum_name: String, variant: String },
}

impl CfdInputDictKey {
    #[must_use]
    pub fn enum_variant(enum_name: impl Into<String>, variant: impl Into<String>) -> Self {
        Self::EnumVariant {
            enum_name: enum_name.into(),
            variant: variant.into(),
        }
    }
}

impl From<&str> for CfdInputDictKey {
    fn from(value: &str) -> Self {
        Self::String(value.to_string())
    }
}

impl From<String> for CfdInputDictKey {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl From<i64> for CfdInputDictKey {
    fn from(value: i64) -> Self {
        Self::Int(value)
    }
}
