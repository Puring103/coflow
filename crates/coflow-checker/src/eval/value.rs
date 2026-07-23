use coflow_cft::{CftConstValue, CftValueType, EnumName, FieldName};
use coflow_data_model::{
    CfdDataModel, CfdDictKey, CfdEnumValue, CfdObject, CfdRecord, CfdRecordId, CfdValue,
};
use coflow_structure::{BudgetExceeded, StructuralBudget, StructureKind, TraversalCursor};
use std::collections::BTreeMap;

use super::collections::{EvalEntries, EvalItems};
use super::location::{ModelCursor, ValueLocation};

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum EvalValue<'a> {
    Model(&'a CfdValue),
    DictKey(&'a CfdDictKey),
    Temporary(TemporaryValue),
    EnumNamespace(EnumName),
    Record(EvalRecordRef),
    Entry(Box<EvalEntry<'a>>),
    Array {
        items: EvalItems,
        element_type: Option<CftValueType>,
    },
    Dict {
        entries: EvalEntries,
        key_type: Option<CftValueType>,
        value_type: Option<CftValueType>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum TemporaryValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Enum(CfdEnumValue),
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum ScalarValue<'a> {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(&'a str),
    Enum(&'a CfdEnumValue),
}

impl<'a> EvalValue<'a> {
    pub(crate) fn from_const(value: &CftConstValue) -> Self {
        Self::Temporary(match value {
            CftConstValue::Int(value) => TemporaryValue::Int(*value),
            CftConstValue::Float(value) => TemporaryValue::Float(*value),
            CftConstValue::Bool(value) => TemporaryValue::Bool(*value),
            CftConstValue::String(value) => TemporaryValue::String(value.clone()),
        })
    }

    pub(crate) fn from_cfd_value(
        value: &'a CfdValue,
        ty: Option<&CftValueType>,
        location: ValueLocation,
        model: &'a CfdDataModel,
        budget: &mut StructuralBudget,
        cursor: TraversalCursor,
    ) -> Result<Self, LocatedBudgetExceeded> {
        let cursor = budget
            .enter(cursor, StructureKind::DataValue, 1)
            .map_err(|error| LocatedBudgetExceeded {
                error,
                location: Box::new(Some(location.clone())),
            })?;
        Ok(match value {
            CfdValue::Null
            | CfdValue::Bool(_)
            | CfdValue::Int(_)
            | CfdValue::Float(_)
            | CfdValue::String(_)
            | CfdValue::Enum(_) => Self::Model(value),
            CfdValue::Object(_) => Self::Record(EvalRecordRef::Resolved(location)),
            CfdValue::Ref(_key) => {
                let resolved = location
                    .storage
                    .ref_site(model)
                    .and_then(|site| model.resolve_ref(&site));
                resolved.map_or_else(
                    || Self::Record(EvalRecordRef::Unresolved),
                    |id| {
                        let target = location.clone().dereference(id);
                        Self::Record(EvalRecordRef::Resolved(target))
                    },
                )
            }
            CfdValue::Array(items) => {
                let element_type = array_element_type(ty).cloned();
                Self::Array {
                    items: EvalItems::ModelArray {
                        storage: location.storage,
                        traversal: cursor,
                        len: items.len(),
                    },
                    element_type,
                }
            }
            CfdValue::Dict(entries) => Self::Dict {
                entries: EvalEntries::new(location.storage, cursor, entries.len()),
                key_type: dict_key_type(ty).cloned(),
                value_type: dict_value_type(ty).cloned(),
            },
        })
    }

    pub(crate) const fn from_dict_key(key: &'a CfdDictKey) -> Self {
        Self::DictKey(key)
    }

    pub(crate) fn actual_type<'model>(&self, model: &'model CfdDataModel) -> Option<&'model str> {
        match self {
            Self::Record(record) => record.actual_type(model),
            _ => None,
        }
    }

    pub(crate) fn collection_len(&self) -> Option<usize> {
        match self {
            Self::Array { items, .. } => Some(items.len()),
            Self::Dict { entries, .. } => Some(entries.len()),
            _ => match self.scalar() {
                Some(ScalarValue::String(value)) => Some(value.chars().count()),
                _ => None,
            },
        }
    }

    pub(crate) fn scalar(&self) -> Option<ScalarValue<'_>> {
        match self {
            Self::Model(value) => scalar_from_cfd(value),
            Self::DictKey(key) => Some(match key {
                CfdDictKey::String(value) => ScalarValue::String(value),
                CfdDictKey::Int(value) => ScalarValue::Int(*value),
                CfdDictKey::Enum(value) => ScalarValue::Enum(value),
            }),
            Self::Temporary(value) => Some(match value {
                TemporaryValue::Null => ScalarValue::Null,
                TemporaryValue::Bool(value) => ScalarValue::Bool(*value),
                TemporaryValue::Int(value) => ScalarValue::Int(*value),
                TemporaryValue::Float(value) => ScalarValue::Float(*value),
                TemporaryValue::String(value) => ScalarValue::String(value),
                TemporaryValue::Enum(value) => ScalarValue::Enum(value),
            }),
            Self::EnumNamespace(_)
            | Self::Record(_)
            | Self::Entry(_)
            | Self::Array { .. }
            | Self::Dict { .. } => None,
        }
    }

    pub(crate) const fn null() -> Self {
        Self::Temporary(TemporaryValue::Null)
    }

    pub(crate) const fn bool(value: bool) -> Self {
        Self::Temporary(TemporaryValue::Bool(value))
    }

    pub(crate) const fn int(value: i64) -> Self {
        Self::Temporary(TemporaryValue::Int(value))
    }

    pub(crate) const fn float(value: f64) -> Self {
        Self::Temporary(TemporaryValue::Float(value))
    }

    pub(crate) fn string(value: impl Into<String>) -> Self {
        Self::Temporary(TemporaryValue::String(value.into()))
    }

    pub(crate) const fn enum_value(value: CfdEnumValue) -> Self {
        Self::Temporary(TemporaryValue::Enum(value))
    }
}

fn scalar_from_cfd(value: &CfdValue) -> Option<ScalarValue<'_>> {
    match value {
        CfdValue::Null => Some(ScalarValue::Null),
        CfdValue::Bool(value) => Some(ScalarValue::Bool(*value)),
        CfdValue::Int(value) => Some(ScalarValue::Int(*value)),
        CfdValue::Float(value) => Some(ScalarValue::Float(*value)),
        CfdValue::String(value) => Some(ScalarValue::String(value)),
        CfdValue::Enum(value) => Some(ScalarValue::Enum(value)),
        CfdValue::Object(_) | CfdValue::Ref(_) | CfdValue::Array(_) | CfdValue::Dict(_) => None,
    }
}

#[derive(Debug)]
pub(crate) struct LocatedBudgetExceeded {
    pub(crate) error: BudgetExceeded,
    pub(crate) location: Box<Option<ValueLocation>>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LocatedEvalValue<'a> {
    pub(crate) value: EvalValue<'a>,
    pub(crate) location: Option<ValueLocation>,
}

impl<'a> LocatedEvalValue<'a> {
    pub(crate) fn new(value: EvalValue<'a>, location: Option<ValueLocation>) -> Self {
        Self { value, location }
    }

    pub(crate) fn value(value: EvalValue<'a>) -> Self {
        Self {
            value,
            location: None,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) enum EvalRecordRef {
    Resolved(ValueLocation),
    RecordSet(ValueLocation),
    /// A `CfdValue::Ref` whose target could not be resolved (target type/key
    /// missing from the model). Reads through this ref return `None`, so
    /// callers surface a check diagnostic instead of crashing.
    Unresolved,
}

impl PartialEq for EvalRecordRef {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Resolved(lhs) | Self::RecordSet(lhs), Self::Resolved(rhs) | Self::RecordSet(rhs)) => lhs.storage == rhs.storage,
            (Self::Unresolved, Self::Unresolved) => true,
            (Self::Resolved(_) | Self::RecordSet(_), Self::Unresolved)
            | (Self::Unresolved, Self::Resolved(_) | Self::RecordSet(_)) => false,
        }
    }
}

impl EvalRecordRef {
    pub(crate) fn fields<'model>(
        &self,
        model: &'model CfdDataModel,
    ) -> Option<&'model BTreeMap<FieldName, CfdValue>> {
        match self {
            Self::Resolved(location) | Self::RecordSet(location) => resolved_object_fields(model, &location.storage),
            Self::Unresolved => None,
        }
    }

    pub(crate) fn actual_type<'model>(&self, model: &'model CfdDataModel) -> Option<&'model str> {
        match self {
            Self::Resolved(location) | Self::RecordSet(location) => resolved_object_type(model, &location.storage),
            Self::Unresolved => None,
        }
    }

    pub(crate) fn key<'model>(&self, model: &'model CfdDataModel) -> Option<&'model str> {
        match self {
            Self::Resolved(location) | Self::RecordSet(location)
                if location.storage.dimension.is_none()
                    && location.storage.path.segments.is_empty() =>
            {
                model.record(location.storage.record).map(CfdRecord::key)
            }
            Self::Resolved(_) | Self::RecordSet(_) => None,
            Self::Unresolved => None,
        }
    }

    pub(crate) fn field<'model>(
        &self,
        model: &'model CfdDataModel,
        field_type: Option<&CftValueType>,
        name: &str,
        budget: &mut StructuralBudget,
    ) -> Result<Option<LocatedEvalValue<'model>>, LocatedBudgetExceeded> {
        let Some(value) = self.fields(model).and_then(|fields| fields.get(name)) else {
            return Ok(None);
        };
        let Some(location) = self.location().map(|location| location.field(name)) else {
            return Ok(None);
        };
        Ok(Some(LocatedEvalValue::new(
            EvalValue::from_cfd_value(
                value,
                field_type,
                location.clone(),
                model,
                budget,
                TraversalCursor::root(),
            )?,
            Some(location),
        )))
    }

    pub(crate) fn location(&self) -> Option<ValueLocation> {
        match self {
            Self::Resolved(location) | Self::RecordSet(location) => Some(location.clone()),
            Self::Unresolved => None,
        }
    }

    pub(crate) fn top_record_id(&self) -> Option<CfdRecordId> {
        match self {
            Self::Resolved(location) | Self::RecordSet(location)
                if location.storage.dimension.is_none()
                    && location.storage.path.segments.is_empty() =>
            {
                Some(location.storage.record)
            }
            Self::Resolved(_) | Self::RecordSet(_) | Self::Unresolved => None,
        }
    }

    pub(crate) const fn is_record_set_handle(&self) -> bool {
        matches!(self, Self::RecordSet(_))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScalarFormat {
    Display,
    Diagnostic,
}

pub(crate) fn format_scalar(value: ScalarValue<'_>, format: ScalarFormat) -> String {
    match value {
        ScalarValue::Null => "null".to_string(),
        ScalarValue::Bool(value) => value.to_string(),
        ScalarValue::Int(value) => value.to_string(),
        ScalarValue::Float(value) => value.to_string(),
        ScalarValue::String(value) => match format {
            ScalarFormat::Display => value.to_string(),
            ScalarFormat::Diagnostic => format!("\"{value}\""),
        },
        ScalarValue::Enum(value) => value.variant.as_deref().map_or_else(
            || format!("{}({})", value.enum_name, value.value),
            |variant| format!("{}.{variant}", value.enum_name),
        ),
    }
}

fn resolved_object_fields<'a>(
    model: &'a CfdDataModel,
    cursor: &ModelCursor,
) -> Option<&'a BTreeMap<FieldName, CfdValue>> {
    if cursor.dimension.is_none() && cursor.path.segments.is_empty() {
        return model.record(cursor.record).map(CfdRecord::fields);
    }
    inline_object(model, cursor).map(CfdObject::fields)
}

fn resolved_object_type<'a>(model: &'a CfdDataModel, cursor: &ModelCursor) -> Option<&'a str> {
    if cursor.dimension.is_none() && cursor.path.segments.is_empty() {
        return model.record(cursor.record).map(CfdRecord::actual_type);
    }
    inline_object(model, cursor).map(CfdObject::actual_type)
}

fn inline_object<'a>(model: &'a CfdDataModel, cursor: &ModelCursor) -> Option<&'a CfdObject> {
    match model_value(model, cursor)? {
        CfdValue::Object(object) => Some(object),
        _ => None,
    }
}

pub(crate) fn model_value<'a>(
    model: &'a CfdDataModel,
    cursor: &ModelCursor,
) -> Option<&'a CfdValue> {
    let record = model.record(cursor.record)?;
    let Some(dimension) = &cursor.dimension else {
        return record.value_at_path(&cursor.path);
    };
    let mut value = &record
        .dimension_field(&dimension.field)?
        .variants
        .get(dimension.variant.as_str())?
        .value;
    for segment in &cursor.path.segments {
        value = match (segment, value) {
            (coflow_data_model::CfdPathSegment::Field(field), CfdValue::Object(object)) => {
                object.fields().get(field.as_str())?
            }
            (coflow_data_model::CfdPathSegment::Index(index), CfdValue::Array(items)) => {
                items.get(*index)?
            }
            (coflow_data_model::CfdPathSegment::DictKey(key), CfdValue::Dict(entries)) => entries
                .iter()
                .find(|(entry_key, _)| coflow_data_model::format_cfd_dict_key(entry_key) == *key)
                .map(|(_, value)| value)?,
            _ => return None,
        };
    }
    Some(value)
}

fn array_element_type(ty: Option<&CftValueType>) -> Option<&CftValueType> {
    match ty {
        Some(CftValueType::Nullable(inner)) => array_element_type(Some(inner)),
        Some(CftValueType::Array(inner)) => Some(inner),
        _ => None,
    }
}

fn dict_value_type(ty: Option<&CftValueType>) -> Option<&CftValueType> {
    match ty {
        Some(CftValueType::Nullable(inner)) => dict_value_type(Some(inner)),
        Some(CftValueType::Dict(_, value)) => Some(value),
        _ => None,
    }
}

fn dict_key_type(ty: Option<&CftValueType>) -> Option<&CftValueType> {
    match ty {
        Some(CftValueType::Nullable(inner)) => dict_key_type(Some(inner)),
        Some(CftValueType::Dict(key, _)) => Some(key),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct EvalEntry<'a> {
    pub(crate) key: Box<EvalValue<'a>>,
    pub(crate) value: EvalValue<'a>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ComparableKey {
    Null,
    Bool(bool),
    Int(i64),
    String(String),
    Enum(CfdEnumValue),
}

pub(crate) fn values_equal(lhs: &EvalValue<'_>, rhs: &EvalValue<'_>) -> bool {
    match (lhs.scalar(), rhs.scalar()) {
        (Some(ScalarValue::Null), Some(ScalarValue::Null)) => true,
        (Some(ScalarValue::Bool(lhs)), Some(ScalarValue::Bool(rhs))) => lhs == rhs,
        (Some(ScalarValue::Int(lhs)), Some(ScalarValue::Int(rhs))) => lhs == rhs,
        (Some(ScalarValue::Float(lhs)), Some(ScalarValue::Float(rhs))) => lhs == rhs,
        (Some(ScalarValue::String(lhs)), Some(ScalarValue::String(rhs))) => lhs == rhs,
        (Some(ScalarValue::Enum(lhs)), Some(ScalarValue::Enum(rhs))) => {
            lhs.enum_name == rhs.enum_name && lhs.value == rhs.value
        }
        _ => match (lhs, rhs) {
            (EvalValue::Record(lhs), EvalValue::Record(rhs)) => lhs == rhs,
            _ => false,
        },
    }
}

pub(crate) fn comparable_key(value: &EvalValue<'_>) -> Option<ComparableKey> {
    match value.scalar()? {
        ScalarValue::Null => Some(ComparableKey::Null),
        ScalarValue::Bool(value) => Some(ComparableKey::Bool(value)),
        ScalarValue::Int(value) => Some(ComparableKey::Int(value)),
        ScalarValue::String(value) => Some(ComparableKey::String(value.to_string())),
        ScalarValue::Enum(value) => Some(ComparableKey::Enum(value.clone())),
        ScalarValue::Float(_) => None,
    }
}

pub(crate) fn dict_key_from_check_value(value: &EvalValue<'_>) -> Option<ComparableKey> {
    match value.scalar()? {
        ScalarValue::Int(_) | ScalarValue::String(_) | ScalarValue::Enum(_) => {
            comparable_key(value)
        }
        _ => None,
    }
}

pub(crate) fn dict_key_matches(key: &CfdDictKey, expected: &ComparableKey) -> bool {
    match (key, expected) {
        (CfdDictKey::String(key), ComparableKey::String(expected)) => key == expected,
        (CfdDictKey::Int(key), ComparableKey::Int(expected)) => key == expected,
        (CfdDictKey::Enum(key), ComparableKey::Enum(expected)) => {
            key.enum_name == expected.enum_name && key.value == expected.value
        }
        _ => false,
    }
}

/// Formats a dict-entry key (already turned into a `EvalValue`) so it can be
/// pushed onto a [`CfdPath`]. Mirrors `CfdPath::dict_key_value` so quantifier-
/// emitted diagnostics use the same key form as data-model diagnostics.
/// Returns `None` when the value is not a valid dict key shape.
pub(crate) fn format_check_key_for_path(value: &EvalValue<'_>) -> Option<String> {
    match value.scalar()? {
        ScalarValue::String(value) => Some(format!("\"{value}\"")),
        ScalarValue::Int(value) => Some(value.to_string()),
        ScalarValue::Enum(value) => Some(match value.variant.as_deref() {
            Some(variant) => format!("{}.{}", value.enum_name, variant),
            None => format!("{}({})", value.enum_name, value.value),
        }),
        ScalarValue::Null | ScalarValue::Bool(_) | ScalarValue::Float(_) => None,
    }
}
