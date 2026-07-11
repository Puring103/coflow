use coflow_cft::{CftConstValue, CftSchemaTypeRef};
use coflow_data_model::{
    CfdDataModel, CfdDictKey, CfdEnumValue, CfdObject, CfdPath, CfdRecord, CfdRecordId, CfdValue,
    RefSite,
};
use coflow_structure::{BudgetExceeded, StructuralBudget, StructureKind, TraversalCursor};
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
        items: CheckItems,
        element_type: Option<CftSchemaTypeRef>,
    },
    Dict {
        entries: CheckEntries,
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

    pub(super) fn from_cfd_value(
        value: &CfdValue,
        ty: Option<&CftSchemaTypeRef>,
        location: ValueLocation,
        model: &CfdDataModel,
        budget: &mut StructuralBudget,
        cursor: TraversalCursor,
    ) -> Result<Self, LocatedBudgetExceeded> {
        let cursor = budget
            .enter(cursor, StructureKind::DataValue, 1)
            .map_err(|error| LocatedBudgetExceeded {
                error,
                location: Some(location.clone()),
            })?;
        Ok(match value {
            CfdValue::Null => Self::Null,
            CfdValue::Bool(value) => Self::Bool(*value),
            CfdValue::Int(value) => Self::Int(*value),
            CfdValue::Float(value) => Self::Float(*value),
            CfdValue::String(value) => Self::String(value.clone()),
            CfdValue::Enum(value) => Self::Enum(value.clone()),
            CfdValue::Object(_) => Self::Record(CheckRecordRef::Resolved(location)),
            CfdValue::Ref(_) => {
                let resolved = model.resolve_effective_ref(&location.storage.ref_site());
                resolved.map_or_else(
                    || Self::Record(CheckRecordRef::Unresolved),
                    |id| {
                        let target = location.clone().dereference(id);
                        Self::Record(CheckRecordRef::Resolved(target))
                    },
                )
            }
            CfdValue::Array(items) => {
                let element_type = array_element_type(ty).cloned();
                Self::Array {
                    items: CheckItems::ModelArray {
                        storage: location.storage,
                        traversal: cursor,
                        len: items.len(),
                    },
                    element_type,
                }
            }
            CfdValue::Dict(entries) => Self::Dict {
                entries: CheckEntries {
                    storage: location.storage,
                    traversal: cursor,
                    len: entries.len(),
                },
                key_type: dict_key_type(ty).cloned(),
                value_type: dict_value_type(ty).cloned(),
            },
        })
    }

    pub(super) fn from_dict_key(key: &CfdDictKey) -> Self {
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

    pub(super) fn collection_len(&self) -> Option<usize> {
        match self {
            Self::Array { items, .. } => Some(items.len()),
            Self::Dict { entries, .. } => Some(entries.len()),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum CheckItems {
    ModelArray {
        storage: ModelCursor,
        traversal: TraversalCursor,
        len: usize,
    },
    DictKeys(CheckEntries),
    DictValues(CheckEntries),
}

impl CheckItems {
    pub(super) const fn len(&self) -> usize {
        match self {
            Self::ModelArray { len, .. } => *len,
            Self::DictKeys(entries) | Self::DictValues(entries) => entries.len(),
        }
    }

    pub(super) fn located_at(
        &self,
        index: usize,
        element_type: Option<&CftSchemaTypeRef>,
        collection_location: Option<&ValueLocation>,
        model: &CfdDataModel,
        budget: &mut StructuralBudget,
    ) -> Result<Option<LocatedCheckValue>, LocatedBudgetExceeded> {
        let projected_location = collection_location.map(|location| location.index(index));
        match self {
            Self::ModelArray {
                storage, traversal, ..
            } => {
                let Some(value) = model_array(model, storage).and_then(|items| items.get(index))
                else {
                    return Ok(None);
                };
                let Some(location) = projected_location else {
                    return Ok(None);
                };
                let value = CheckValue::from_cfd_value(
                    value,
                    element_type,
                    location.clone(),
                    model,
                    budget,
                    *traversal,
                )?;
                Ok(Some(LocatedCheckValue::new(value, Some(location))))
            }
            Self::DictKeys(entries) => Ok(entries.key_at(model, index).map(|key| {
                LocatedCheckValue::new(CheckValue::from_dict_key(key), projected_location)
            })),
            Self::DictValues(entries) => {
                entries.projected_value_at(index, element_type, projected_location, model, budget)
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct CheckEntries {
    storage: ModelCursor,
    traversal: TraversalCursor,
    len: usize,
}

impl CheckEntries {
    pub(super) const fn len(&self) -> usize {
        self.len
    }

    fn key_at<'a>(&self, model: &'a CfdDataModel, index: usize) -> Option<&'a CfdDictKey> {
        self.model_entry_at(model, index).map(|(key, _)| key)
    }

    pub(super) fn located_entry_at(
        &self,
        index: usize,
        _key_type: Option<&CftSchemaTypeRef>,
        value_type: Option<&CftSchemaTypeRef>,
        collection_location: Option<&ValueLocation>,
        model: &CfdDataModel,
        budget: &mut StructuralBudget,
    ) -> Result<Option<LocatedCheckValue>, LocatedBudgetExceeded> {
        let Some((key, value)) =
            model_dict(model, &self.storage).and_then(|items| items.get(index))
        else {
            return Ok(None);
        };
        let storage_location = ValueLocation {
            storage: self.storage.dict_key_value(key),
            blame: collection_location.map_or_else(
                || self.storage.dict_key_value(key),
                |location| location.blame.dict_key_value(key),
            ),
            references: collection_location
                .map(|location| location.references.clone())
                .unwrap_or_default(),
        };
        let value = CheckValue::from_cfd_value(
            value,
            value_type,
            storage_location,
            model,
            budget,
            self.traversal,
        )?;
        let key = CheckValue::from_dict_key(key);
        let key_label = format_check_key_for_path(&key).unwrap_or_else(|| index.to_string());
        let location = collection_location.map(|location| location.dict_key(key_label));
        Ok(Some(LocatedCheckValue::new(
            CheckValue::Entry(Box::new(CheckEntry {
                key: Box::new(key),
                value,
            })),
            location,
        )))
    }

    fn projected_value_at(
        &self,
        index: usize,
        value_type: Option<&CftSchemaTypeRef>,
        projected_location: Option<ValueLocation>,
        model: &CfdDataModel,
        budget: &mut StructuralBudget,
    ) -> Result<Option<LocatedCheckValue>, LocatedBudgetExceeded> {
        let Some((key, value)) =
            model_dict(model, &self.storage).and_then(|items| items.get(index))
        else {
            return Ok(None);
        };
        let Some(projected_location) = projected_location else {
            return Ok(None);
        };
        let storage_location = ValueLocation {
            storage: self.storage.dict_key_value(key),
            blame: projected_location.blame.clone(),
            references: projected_location.references.clone(),
        };
        let value = CheckValue::from_cfd_value(
            value,
            value_type,
            storage_location,
            model,
            budget,
            self.traversal,
        )?;
        Ok(Some(LocatedCheckValue::new(
            value,
            Some(projected_location),
        )))
    }

    pub(super) fn model_entry_at<'a>(
        &self,
        model: &'a CfdDataModel,
        index: usize,
    ) -> Option<(&'a CfdDictKey, &'a CfdValue)> {
        model_dict(model, &self.storage)
            .and_then(|items| items.get(index))
            .map(|(key, value)| (key, value))
    }
}

fn model_array<'a>(model: &'a CfdDataModel, cursor: &ModelCursor) -> Option<&'a [CfdValue]> {
    match model.record(cursor.record)?.value_at_path(&cursor.path)? {
        CfdValue::Array(items) => Some(items),
        _ => None,
    }
}

fn model_dict<'a>(
    model: &'a CfdDataModel,
    cursor: &ModelCursor,
) -> Option<&'a [(CfdDictKey, CfdValue)]> {
    match model.record(cursor.record)?.value_at_path(&cursor.path)? {
        CfdValue::Dict(entries) => Some(entries),
        _ => None,
    }
}

#[derive(Debug)]
pub(super) struct LocatedBudgetExceeded {
    pub(super) error: BudgetExceeded,
    pub(super) location: Option<ValueLocation>,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct LocatedCheckValue {
    pub(super) value: CheckValue,
    pub(super) location: Option<ValueLocation>,
}

impl LocatedCheckValue {
    pub(super) fn new(value: CheckValue, location: Option<ValueLocation>) -> Self {
        Self { value, location }
    }

    pub(super) fn value(value: CheckValue) -> Self {
        Self {
            value,
            location: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ModelCursor {
    pub(super) record: CfdRecordId,
    pub(super) path: CfdPath,
}

impl ModelCursor {
    pub(super) fn root(record: CfdRecordId) -> Self {
        Self {
            record,
            path: CfdPath::root(),
        }
    }

    pub(super) fn field(&self, name: impl Into<String>) -> Self {
        Self {
            record: self.record,
            path: self.path.clone().field(name),
        }
    }

    pub(super) fn index(&self, index: usize) -> Self {
        Self {
            record: self.record,
            path: self.path.clone().index(index),
        }
    }

    pub(super) fn dict_key_value(&self, key: &CfdDictKey) -> Self {
        Self {
            record: self.record,
            path: self.path.clone().dict_key_value(key),
        }
    }

    pub(super) fn dict_key(&self, key: impl Into<String>) -> Self {
        Self {
            record: self.record,
            path: self.path.clone().dict_key(key),
        }
    }

    pub(super) fn ref_site(&self) -> RefSite {
        RefSite::new(self.record, self.path.clone())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ValueLocation {
    pub(super) storage: ModelCursor,
    pub(super) blame: ModelCursor,
    pub(super) references: Vec<ModelCursor>,
}

impl ValueLocation {
    pub(super) fn root(record: CfdRecordId) -> Self {
        let cursor = ModelCursor::root(record);
        Self {
            storage: cursor.clone(),
            blame: cursor,
            references: Vec::new(),
        }
    }

    pub(super) fn field(&self, name: impl Into<String>) -> Self {
        let name = name.into();
        Self {
            storage: self.storage.field(name.clone()),
            blame: self.blame.field(name),
            references: self.references.clone(),
        }
    }

    pub(super) fn index(&self, index: usize) -> Self {
        Self {
            storage: self.storage.index(index),
            blame: self.blame.index(index),
            references: self.references.clone(),
        }
    }

    pub(super) fn dict_key_value(&self, key: &CfdDictKey) -> Self {
        Self {
            storage: self.storage.dict_key_value(key),
            blame: self.blame.dict_key_value(key),
            references: self.references.clone(),
        }
    }

    pub(super) fn dict_key(&self, key: impl Into<String>) -> Self {
        let key = key.into();
        Self {
            storage: self.storage.dict_key(key.clone()),
            blame: self.blame.dict_key(key),
            references: self.references.clone(),
        }
    }

    pub(super) fn backed_by(&self, storage: ModelCursor) -> Self {
        Self {
            storage,
            blame: self.blame.clone(),
            references: self.references.clone(),
        }
    }

    fn dereference(mut self, target: CfdRecordId) -> Self {
        self.references.push(self.blame);
        let target = ModelCursor::root(target);
        Self {
            storage: target.clone(),
            blame: target,
            references: self.references,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum CheckRecordRef {
    Resolved(ValueLocation),
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
            Self::Resolved(location) => resolved_object_fields(model, &location.storage),
            Self::Unresolved => None,
        }
    }

    pub(super) fn actual_type<'a>(&'a self, model: &'a CfdDataModel) -> Option<&'a str> {
        match self {
            Self::Resolved(location) => resolved_object_type(model, &location.storage),
            Self::Unresolved => None,
        }
    }

    pub(super) fn key<'a>(&'a self, model: &'a CfdDataModel) -> Option<&'a str> {
        match self {
            Self::Resolved(location) if location.storage.path.segments.is_empty() => {
                model.record(location.storage.record).map(CfdRecord::key)
            }
            Self::Resolved(_) => None,
            Self::Unresolved => None,
        }
    }

    pub(super) fn field(
        &self,
        model: &CfdDataModel,
        field_type: Option<&CftSchemaTypeRef>,
        name: &str,
        budget: &mut StructuralBudget,
    ) -> Result<Option<LocatedCheckValue>, LocatedBudgetExceeded> {
        let Some(value) = self.fields(model).and_then(|fields| fields.get(name)) else {
            return Ok(None);
        };
        let Some(location) = self.location().map(|location| location.field(name)) else {
            return Ok(None);
        };
        Ok(Some(LocatedCheckValue::new(
            CheckValue::from_cfd_value(
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

    pub(super) fn location(&self) -> Option<ValueLocation> {
        match self {
            Self::Resolved(location) => Some(location.clone()),
            Self::Unresolved => None,
        }
    }

    pub(super) fn top_record_id(&self) -> Option<CfdRecordId> {
        match self {
            Self::Resolved(location) if location.storage.path.segments.is_empty() => {
                Some(location.storage.record)
            }
            Self::Resolved(_) | Self::Unresolved => None,
        }
    }
}

fn resolved_object_fields<'a>(
    model: &'a CfdDataModel,
    cursor: &ModelCursor,
) -> Option<&'a BTreeMap<String, CfdValue>> {
    if cursor.path.segments.is_empty() {
        return model.record(cursor.record).map(CfdRecord::fields);
    }
    inline_object(model, cursor).map(CfdObject::fields)
}

fn resolved_object_type<'a>(model: &'a CfdDataModel, cursor: &ModelCursor) -> Option<&'a str> {
    if cursor.path.segments.is_empty() {
        return model.record(cursor.record).map(CfdRecord::actual_type);
    }
    inline_object(model, cursor).map(CfdObject::actual_type)
}

fn inline_object<'a>(model: &'a CfdDataModel, cursor: &ModelCursor) -> Option<&'a CfdObject> {
    match model.record(cursor.record)?.value_at_path(&cursor.path)? {
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

pub(super) fn dict_key_matches(key: &CfdDictKey, expected: &ComparableKey) -> bool {
    match (key, expected) {
        (CfdDictKey::String(key), ComparableKey::String(expected)) => key == expected,
        (CfdDictKey::Int(key), ComparableKey::Int(expected)) => key == expected,
        (CfdDictKey::Enum(key), ComparableKey::Enum(expected)) => {
            key.enum_name == expected.enum_name && key.value == expected.value
        }
        _ => false,
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
