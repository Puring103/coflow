use coflow_cft::{CftConstValue, CftSchemaTypeRef};
use coflow_data_model::{
    CfdDataModel, CfdDictKey, CfdEnumValue, CfdObject, CfdPath, CfdRecord, CfdRecordId, CfdValue,
    RefSite,
};
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
    Array {
        items: Vec<CheckValue>,
        element_type: Option<CftSchemaTypeRef>,
    },
    Dict {
        entries: Vec<CheckEntry>,
        key_type: Option<CftSchemaTypeRef>,
        value_type: Option<CftSchemaTypeRef>,
    },
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

    pub(super) fn from_cfd_value_with_path(
        value: &CfdValue,
        ty: Option<&CftSchemaTypeRef>,
        path: Option<CfdPath>,
        model: &CfdDataModel,
        site: Option<RefSite>,
    ) -> Self {
        match value {
            CfdValue::Null => Self::Null,
            CfdValue::Bool(value) => Self::Bool(*value),
            CfdValue::Int(value) => Self::Int(*value),
            CfdValue::Float(value) => Self::Float(*value),
            CfdValue::String(value) => Self::String(value.clone()),
            CfdValue::Enum(value) => Self::Enum(value.clone()),
            CfdValue::Object(_) => site.map_or_else(
                || Self::Record(CheckRecordRef::Unresolved),
                |site| Self::Record(CheckRecordRef::Inline { site, path }),
            ),
            CfdValue::Ref(_) => {
                let resolved = site
                    .as_ref()
                    .and_then(|site| model.resolve_effective_ref(site));
                resolved.map_or_else(
                    || Self::Record(CheckRecordRef::Unresolved),
                    |id| Self::Record(CheckRecordRef::Top(id)),
                )
            }
            CfdValue::Array(items) => {
                let element_type = array_element_type(ty).cloned();
                Self::Array {
                    items: items
                        .iter()
                        .enumerate()
                        .map(|(index, item)| {
                            Self::from_cfd_value_with_path(
                                item,
                                array_element_type(ty),
                                path.clone().map(|path| path.index(index)),
                                model,
                                site.clone().map(|site| {
                                    RefSite::new(site.host, site.path.index(index))
                                }),
                            )
                        })
                        .collect(),
                    element_type,
                }
            }
            CfdValue::Dict(entries) => Self::Dict {
                entries: entries
                    .iter()
                    .map(|(key, value)| CheckEntry {
                        key: Box::new(Self::from_dict_key(key)),
                        value: Self::from_cfd_value_with_path(
                            value,
                            dict_value_type(ty),
                            path.clone().map(|path| path.dict_key_value(key)),
                            model,
                            site.clone().map(|site| {
                                RefSite::new(site.host, site.path.dict_key_value(key))
                            }),
                        ),
                    })
                    .collect(),
                key_type: dict_key_type(ty).cloned(),
                value_type: dict_value_type(ty).cloned(),
            },
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
pub(super) struct LocatedCheckValue {
    pub(super) value: CheckValue,
    pub(super) path: Option<CfdPath>,
}

impl LocatedCheckValue {
    pub(super) fn new(value: CheckValue, path: Option<CfdPath>) -> Self {
        Self { value, path }
    }

    pub(super) fn value(value: CheckValue) -> Self {
        Self { value, path: None }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum CheckRecordRef {
    Top(CfdRecordId),
    Inline {
        site: RefSite,
        path: Option<CfdPath>,
    },
    /// A `CfdValue::Ref` whose target could not be resolved (target type/key
    /// missing from the model). Reads through this ref return `None`, so
    /// callers surface a check diagnostic instead of crashing.
    Unresolved,
}

impl CheckRecordRef {
    pub(super) fn fields<'a>(
        &'a self,
        model: &'a CfdDataModel,
    ) -> Option<&'a BTreeMap<String, CfdValue>> {
        match self {
            Self::Top(id) => model.record(*id).map(CfdRecord::fields),
            Self::Inline { site, .. } => inline_object(model, site).map(CfdObject::fields),
            Self::Unresolved => None,
        }
    }

    pub(super) fn actual_type<'a>(&'a self, model: &'a CfdDataModel) -> Option<&'a str> {
        match self {
            Self::Top(id) => model.record(*id).map(CfdRecord::actual_type),
            Self::Inline { site, .. } => inline_object(model, site).map(CfdObject::actual_type),
            Self::Unresolved => None,
        }
    }

    pub(super) fn key<'a>(&'a self, model: &'a CfdDataModel) -> Option<&'a str> {
        match self {
            Self::Top(id) => model.record(*id).map(CfdRecord::key),
            Self::Inline { .. } => None,
            Self::Unresolved => None,
        }
    }

    pub(super) fn field(
        &self,
        model: &CfdDataModel,
        field_type: Option<&CftSchemaTypeRef>,
        name: &str,
    ) -> Option<LocatedCheckValue> {
        let value = self.fields(model)?.get(name)?;
        let path = self.path().map(|path| path.field(name.to_string()));
        let site = self
            .value_site()
            .map(|site| RefSite::new(site.host, site.path.field(name)));
        Some(LocatedCheckValue::new(
            CheckValue::from_cfd_value_with_path(value, field_type, path.clone(), model, site),
            path,
        ))
    }

    pub(super) fn path(&self) -> Option<CfdPath> {
        match self {
            Self::Top(_) | Self::Unresolved => Some(CfdPath::root()),
            Self::Inline { path, .. } => path.clone(),
        }
    }

    fn value_site(&self) -> Option<RefSite> {
        match self {
            Self::Top(id) => Some(RefSite::new(*id, CfdPath::root())),
            Self::Inline { site, .. } => Some(site.clone()),
            Self::Unresolved => None,
        }
    }
}

fn inline_object<'a>(model: &'a CfdDataModel, site: &RefSite) -> Option<&'a CfdObject> {
    match model.record(site.host)?.value_at_path(&site.path)? {
        CfdValue::Object(object) => Some(object),
        _ => None,
    }
}

fn array_element_type(ty: Option<&CftSchemaTypeRef>) -> Option<&CftSchemaTypeRef> {
    match ty {
        Some(CftSchemaTypeRef::Nullable(inner)) => array_element_type(Some(inner)),
        Some(CftSchemaTypeRef::Array(inner)) => Some(inner),
        _ => None,
    }
}

fn dict_value_type(ty: Option<&CftSchemaTypeRef>) -> Option<&CftSchemaTypeRef> {
    match ty {
        Some(CftSchemaTypeRef::Nullable(inner)) => dict_value_type(Some(inner)),
        Some(CftSchemaTypeRef::Dict(_, value)) => Some(value),
        _ => None,
    }
}

fn dict_key_type(ty: Option<&CftSchemaTypeRef>) -> Option<&CftSchemaTypeRef> {
    match ty {
        Some(CftSchemaTypeRef::Nullable(inner)) => dict_key_type(Some(inner)),
        Some(CftSchemaTypeRef::Dict(key, _)) => Some(key),
        _ => None,
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

/// Formats a dict-entry key (already turned into a `CheckValue`) so it can be
/// pushed onto a [`CfdPath`]. Mirrors `CfdPath::dict_key_value` so quantifier-
/// emitted diagnostics use the same key form as data-model diagnostics.
/// Returns `None` when the value is not a valid dict key shape.
pub(super) fn format_check_key_for_path(value: &CheckValue) -> Option<String> {
    match value {
        CheckValue::String(value) => Some(format!("\"{value}\"")),
        CheckValue::Int(value) => Some(value.to_string()),
        CheckValue::Enum(value) => Some(match value.variant.as_deref() {
            Some(variant) => format!("{}.{}", value.enum_name, variant),
            None => format!("{}({})", value.enum_name, value.value),
        }),
        _ => None,
    }
}
