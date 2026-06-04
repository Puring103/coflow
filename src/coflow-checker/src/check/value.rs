use coflow_cft::CftConstValue;
use coflow_data_model::{CfdDataModel, CfdDictKey, CfdEnumValue, CfdRecord, CfdRecordId, CfdValue};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub(super) enum CheckValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Enum(CfdEnumValue),
    EnumNamespace(String),
    Record(CheckRecordRef),
    Entry(Box<CheckEntry>),
    Array(Vec<CheckValue>),
    Dict(Vec<CheckEntry>),
}

impl CheckValue {
    pub(super) fn from_const(value: &CftConstValue) -> Self {
        match value {
            CftConstValue::Int(value) => Self::Int(*value),
            CftConstValue::Float(value) => Self::Float(*value),
            CftConstValue::Bool(value) => Self::Bool(*value),
            CftConstValue::String(value) => Self::String(value.clone()),
        }
    }

    pub(super) fn field(&self, model: &CfdDataModel, name: &str) -> Option<CheckValue> {
        let Self::Record(record) = self else {
            return None;
        };
        record
            .fields(model)?
            .get(name)
            .map(CheckValue::from_cfd_value)
    }

    fn from_cfd_value(value: &CfdValue) -> Self {
        match value {
            CfdValue::Null => Self::Null,
            CfdValue::Bool(value) => Self::Bool(*value),
            CfdValue::Int(value) => Self::Int(*value),
            CfdValue::Float(value) => Self::Float(*value),
            CfdValue::String(value) => Self::String(value.clone()),
            CfdValue::Enum(value) => Self::Enum(value.clone()),
            CfdValue::Object(record) => {
                Self::Record(CheckRecordRef::Inline(record.as_ref().clone()))
            }
            CfdValue::Ref { target, .. } => Self::Record(CheckRecordRef::Top(*target)),
            CfdValue::Array(items) => Self::Array(items.iter().map(Self::from_cfd_value).collect()),
            CfdValue::Dict(entries) => Self::Dict(
                entries
                    .iter()
                    .map(|(key, value)| CheckEntry {
                        key: Box::new(Self::from_dict_key(key)),
                        value: Self::from_cfd_value(value),
                    })
                    .collect(),
            ),
        }
    }

    fn from_dict_key(key: &CfdDictKey) -> Self {
        match key {
            CfdDictKey::String(value) => Self::String(value.clone()),
            CfdDictKey::Int(value) => Self::Int(*value),
            CfdDictKey::Enum(value) => Self::Enum(value.clone()),
        }
    }

    pub(super) fn actual_type<'a>(&'a self, model: &'a CfdDataModel) -> Option<&'a str> {
        match self {
            Self::Record(record) => record.actual_type(model),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum CheckRecordRef {
    Top(CfdRecordId),
    Inline(CfdRecord),
}

impl CheckRecordRef {
    pub(super) fn fields<'a>(
        &'a self,
        model: &'a CfdDataModel,
    ) -> Option<&'a BTreeMap<String, CfdValue>> {
        match self {
            Self::Top(id) => model.record(*id).map(|record| &record.fields),
            Self::Inline(record) => Some(&record.fields),
        }
    }

    pub(super) fn actual_type<'a>(&'a self, model: &'a CfdDataModel) -> Option<&'a str> {
        match self {
            Self::Top(id) => model.record(*id).map(|record| record.actual_type.as_str()),
            Self::Inline(record) => Some(&record.actual_type),
        }
    }

    pub(super) fn field(&self, model: &CfdDataModel, name: &str) -> Option<CheckValue> {
        self.fields(model)?
            .get(name)
            .map(CheckValue::from_cfd_value)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct CheckEntry {
    pub(super) key: Box<CheckValue>,
    pub(super) value: CheckValue,
}

impl CheckEntry {
    pub(super) fn key_key(&self) -> Option<ComparableKey> {
        comparable_key(&self.key)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum ComparableKey {
    Null,
    Bool(bool),
    Int(i64),
    String(String),
    Enum(CfdEnumValue),
}

pub(super) fn values_equal(lhs: &CheckValue, rhs: &CheckValue) -> bool {
    match (lhs, rhs) {
        (CheckValue::Null, CheckValue::Null) => true,
        (CheckValue::Bool(lhs), CheckValue::Bool(rhs)) => lhs == rhs,
        (CheckValue::Int(lhs), CheckValue::Int(rhs)) => lhs == rhs,
        (CheckValue::Float(lhs), CheckValue::Float(rhs)) => lhs == rhs,
        (CheckValue::String(lhs), CheckValue::String(rhs)) => lhs == rhs,
        (CheckValue::Enum(lhs), CheckValue::Enum(rhs)) => {
            lhs.enum_name == rhs.enum_name && lhs.value == rhs.value
        }
        (CheckValue::Record(lhs), CheckValue::Record(rhs)) => lhs == rhs,
        _ => false,
    }
}

pub(super) fn comparable_key(value: &CheckValue) -> Option<ComparableKey> {
    match value {
        CheckValue::Null => Some(ComparableKey::Null),
        CheckValue::Bool(value) => Some(ComparableKey::Bool(*value)),
        CheckValue::Int(value) => Some(ComparableKey::Int(*value)),
        CheckValue::String(value) => Some(ComparableKey::String(value.clone())),
        CheckValue::Enum(value) => Some(ComparableKey::Enum(value.clone())),
        _ => None,
    }
}

pub(super) fn dict_key_from_check_value(value: &CheckValue) -> Option<ComparableKey> {
    match value {
        CheckValue::Int(_) | CheckValue::String(_) | CheckValue::Enum(_) => comparable_key(value),
        _ => None,
    }
}
