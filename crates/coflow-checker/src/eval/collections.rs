use crate::limits::{EvaluationBudget, EvaluationCursor, EvaluationKind};
use coflow_language::cft::{CftValueType, FieldName, VariantName};
use coflow_model::{CfdDataModel, CfdDictKey, CfdRecordHandle, CfdValue};

use super::location::{ModelCursor, ValueLocation};
use super::value::{
    format_check_key_for_path, model_value, EvalEntry, EvalValue, LocatedBudgetExceeded,
    LocatedEvalValue,
};

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum EvalItems {
    ModelArray {
        storage: ModelCursor,
        traversal: EvaluationCursor,
        len: usize,
    },
    DictKeys(EvalEntries),
    DictValues(EvalEntries),
    Records(std::sync::Arc<[CfdRecordHandle]>),
}

impl EvalItems {
    pub(crate) fn len(&self) -> usize {
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
        budget: &mut EvaluationBudget,
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
            Self::DictKeys(entries) => Ok(entries
                .key_at(model, index, element_type)
                .map(|key| LocatedEvalValue::new(key, projected_location))),
            Self::DictValues(entries) => {
                entries.projected_value_at(index, element_type, projected_location, model, budget)
            }
            Self::Records(records) => {
                let Some(record) = records.get(index).copied() else {
                    return Ok(None);
                };
                let location = ValueLocation::root(record);
                budget
                    .enter(EvaluationCursor::root(), EvaluationKind::DataValue, 1)
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
    traversal: EvaluationCursor,
    len: usize,
    dimension: Option<DimensionEntries>,
}

#[derive(Debug, Clone, PartialEq)]
struct DimensionEntries {
    base: ValueLocation,
    field: FieldName,
    variants: Vec<VariantName>,
}

impl EvalEntries {
    pub(crate) const fn new(storage: ModelCursor, traversal: EvaluationCursor, len: usize) -> Self {
        Self {
            storage,
            traversal,
            len,
            dimension: None,
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.len
    }

    pub(crate) fn dimension(
        base: ValueLocation,
        field: FieldName,
        variants: Vec<VariantName>,
    ) -> Self {
        Self {
            storage: base.storage.clone(),
            traversal: EvaluationCursor::root(),
            len: variants.len(),
            dimension: Some(DimensionEntries {
                base,
                field,
                variants,
            }),
        }
    }

    pub(crate) fn key_at<'a>(
        &self,
        model: &'a CfdDataModel,
        index: usize,
        key_type: Option<&CftValueType>,
    ) -> Option<EvalValue<'a>> {
        if let Some(dimension) = &self.dimension {
            return dimension
                .variants
                .get(index)
                .map(|variant| EvalValue::string(variant.as_str()));
        }
        model_dict(model, &self.storage)
            .and_then(|items| items.get(index))
            .map(|(key, _)| EvalValue::from_dict_key(key, key_type))
    }

    // 变体集合只保存选择坐标，读取时复用模型里的值，避免复制业务数据。
    fn dimension_value_at<'a>(
        &self,
        index: usize,
        value_type: Option<&CftValueType>,
        model: &'a CfdDataModel,
        budget: &mut EvaluationBudget,
    ) -> Result<Option<LocatedEvalValue<'a>>, LocatedBudgetExceeded> {
        let Some(dimension) = &self.dimension else {
            return Ok(None);
        };
        let Some(variant) = dimension.variants.get(index) else {
            return Ok(None);
        };
        let record = dimension.base.storage.record;
        let Some(value) = model.dimension_value(record, dimension.field.as_str(), variant.as_str())
        else {
            return Ok(None);
        };
        let storage = ModelCursor::dimension(record, dimension.field.as_str(), variant.as_str());
        let location = ValueLocation {
            storage: storage.clone(),
            blame: storage,
            references: dimension.base.references.clone(),
        };
        let value = EvalValue::from_cfd_value(
            value,
            value_type,
            location.clone(),
            model,
            budget,
            self.traversal,
        )?;
        Ok(Some(LocatedEvalValue::new(value, Some(location))))
    }

    pub(crate) fn located_entry_at<'a>(
        &self,
        index: usize,
        key_type: Option<&CftValueType>,
        value_type: Option<&CftValueType>,
        collection_location: Option<&ValueLocation>,
        model: &'a CfdDataModel,
        budget: &mut EvaluationBudget,
    ) -> Result<Option<LocatedEvalValue<'a>>, LocatedBudgetExceeded> {
        if self.dimension.is_some() {
            let Some(value) = self.dimension_value_at(index, value_type, model, budget)? else {
                return Ok(None);
            };
            let Some(key) = self.key_at(model, index, key_type) else {
                return Ok(None);
            };
            return Ok(Some(LocatedEvalValue::new(
                EvalValue::Entry(Box::new(EvalEntry {
                    key: Box::new(key),
                    value: value.value,
                })),
                value.location,
            )));
        }
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
        let key = EvalValue::from_dict_key(key, key_type);
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
        budget: &mut EvaluationBudget,
    ) -> Result<Option<LocatedEvalValue<'a>>, LocatedBudgetExceeded> {
        if self.dimension.is_some() {
            return self.dimension_value_at(index, value_type, model, budget);
        }
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
}

fn model_array<'a>(model: &'a CfdDataModel, cursor: &ModelCursor) -> Option<&'a [CfdValue]> {
    match super::value::transparent_value(model_value(model, cursor)?) {
        CfdValue::Array(items) => Some(items),
        _ => None,
    }
}

fn model_dict<'a>(
    model: &'a CfdDataModel,
    cursor: &ModelCursor,
) -> Option<&'a [(CfdDictKey, CfdValue)]> {
    match super::value::transparent_value(model_value(model, cursor)?) {
        CfdValue::Dict(entries) => Some(entries),
        _ => None,
    }
}
