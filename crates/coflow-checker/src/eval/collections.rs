use coflow_cft::CftValueType;
use coflow_data_model::{CfdDataModel, CfdDictKey, CfdRecordId, CfdValue};
use coflow_structure::{StructuralBudget, StructureKind, TraversalCursor};

use super::location::{ModelCursor, ValueLocation};
use super::value::{
    format_check_key_for_path, model_value, EvalEntry, EvalValue, LocatedBudgetExceeded,
    LocatedEvalValue,
};

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum EvalItems {
    ModelArray {
        storage: ModelCursor,
        traversal: TraversalCursor,
        len: usize,
    },
    DictKeys(EvalEntries),
    DictValues(EvalEntries),
    Records(Vec<CfdRecordId>),
}

impl EvalItems {
    pub(crate) const fn len(&self) -> usize {
        match self {
            Self::ModelArray { len, .. } => *len,
            Self::DictKeys(entries) | Self::DictValues(entries) => entries.len(),
            Self::Records(records) => records.len(),
        }
    }

    pub(crate) fn located_at<'a>(
        &self,
        index: usize,
        element_type: Option<&CftValueType>,
        collection_location: Option<&ValueLocation>,
        model: &'a CfdDataModel,
        budget: &mut StructuralBudget,
    ) -> Result<Option<LocatedEvalValue<'a>>, LocatedBudgetExceeded> {
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
                let value = EvalValue::from_cfd_value(
                    value,
                    element_type,
                    location.clone(),
                    model,
                    budget,
                    *traversal,
                )?;
                Ok(Some(LocatedEvalValue::new(value, Some(location))))
            }
            Self::DictKeys(entries) => Ok(entries.key_at(model, index).map(|key| {
                LocatedEvalValue::new(EvalValue::from_dict_key(key), projected_location)
            })),
            Self::DictValues(entries) => {
                entries.projected_value_at(index, element_type, projected_location, model, budget)
            }
            Self::Records(records) => {
                let Some(record) = records.get(index).copied() else {
                    return Ok(None);
                };
                let location = ValueLocation::root(record);
                budget
                    .enter(TraversalCursor::root(), StructureKind::DataValue, 1)
                    .map_err(|error| LocatedBudgetExceeded {
                        error,
                        location: Box::new(Some(location.clone())),
                    })?;
                Ok(Some(LocatedEvalValue::new(
                    EvalValue::Record(super::value::EvalRecordRef::RecordSet(location.clone())),
                    Some(location),
                )))
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct EvalEntries {
    storage: ModelCursor,
    traversal: TraversalCursor,
    len: usize,
}

impl EvalEntries {
    pub(crate) const fn new(storage: ModelCursor, traversal: TraversalCursor, len: usize) -> Self {
        Self {
            storage,
            traversal,
            len,
        }
    }

    pub(crate) const fn len(&self) -> usize {
        self.len
    }

    fn key_at<'a>(&self, model: &'a CfdDataModel, index: usize) -> Option<&'a CfdDictKey> {
        self.model_entry_at(model, index).map(|(key, _)| key)
    }

    pub(crate) fn located_entry_at<'a>(
        &self,
        index: usize,
        _key_type: Option<&CftValueType>,
        value_type: Option<&CftValueType>,
        collection_location: Option<&ValueLocation>,
        model: &'a CfdDataModel,
        budget: &mut StructuralBudget,
    ) -> Result<Option<LocatedEvalValue<'a>>, LocatedBudgetExceeded> {
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
        let value = EvalValue::from_cfd_value(
            value,
            value_type,
            storage_location,
            model,
            budget,
            self.traversal,
        )?;
        let key = EvalValue::from_dict_key(key);
        let key_label = format_check_key_for_path(&key).unwrap_or_else(|| index.to_string());
        let location = collection_location.map(|location| location.dict_key(key_label));
        Ok(Some(LocatedEvalValue::new(
            EvalValue::Entry(Box::new(EvalEntry {
                key: Box::new(key),
                value,
            })),
            location,
        )))
    }

    fn projected_value_at<'a>(
        &self,
        index: usize,
        value_type: Option<&CftValueType>,
        projected_location: Option<ValueLocation>,
        model: &'a CfdDataModel,
        budget: &mut StructuralBudget,
    ) -> Result<Option<LocatedEvalValue<'a>>, LocatedBudgetExceeded> {
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
        let value = EvalValue::from_cfd_value(
            value,
            value_type,
            storage_location,
            model,
            budget,
            self.traversal,
        )?;
        Ok(Some(LocatedEvalValue::new(value, Some(projected_location))))
    }

    pub(crate) fn model_entry_at<'a>(
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
    match model_value(model, cursor)? {
        CfdValue::Array(items) => Some(items),
        _ => None,
    }
}

fn model_dict<'a>(
    model: &'a CfdDataModel,
    cursor: &ModelCursor,
) -> Option<&'a [(CfdDictKey, CfdValue)]> {
    match model_value(model, cursor)? {
        CfdValue::Dict(entries) => Some(entries),
        _ => None,
    }
}
