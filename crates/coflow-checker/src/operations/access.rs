use coflow_cft::{CftSchema, CftValueType};
use coflow_data_model::CfdDataModel;
use coflow_data_model::CfdErrorCode;
use coflow_structure::StructuralBudget;

use super::diagnostics::format_value_for_message;
use super::ops::{OpsError, OpsResult};
use super::value::{
    dict_key_from_check_value, dict_key_matches, EvalRecordRef, EvalValue, LocatedBudgetExceeded,
    LocatedEvalValue, ScalarValue, ValueLocation,
};

pub(crate) fn index_value<'model>(
    target: LocatedEvalValue<'model>,
    index: LocatedEvalValue<'model>,
    model: &'model CfdDataModel,
    budget: &mut StructuralBudget,
) -> OpsResult<LocatedEvalValue<'model>> {
    if matches!(target.value.scalar(), Some(ScalarValue::Null)) {
        return Err(OpsError::new(
            CfdErrorCode::CheckNullAccess,
            target.location,
            format!(
                "不能索引 null: 尝试在 null 上读取 [{}]",
                format_value_for_message(&index.value)
            ),
        ));
    }
    match target.value {
        EvalValue::Array {
            items,
            element_type,
        } => {
            let index_location = index.location.clone();
            let index_kind = index.value.clone();
            let Some(ScalarValue::Int(idx)) = index.value.scalar() else {
                return Err(OpsError::eval_type(
                    index_location,
                    format!(
                        "数组索引不是 int: 实际为 {}",
                        format_value_for_message(&index_kind)
                    ),
                ));
            };
            let len = items.len();
            let Ok(idx_us) = usize::try_from(idx) else {
                return Err(OpsError::new(
                    CfdErrorCode::CheckIndexOutOfBounds,
                    target.location,
                    format!("数组索引为负数: 实际为 {idx}，长度为 {len}"),
                ));
            };
            items
                .located_at(
                    idx_us,
                    element_type.as_ref(),
                    target.location.as_ref(),
                    model,
                    budget,
                )
                .map_err(budget_error)?
                .ok_or_else(|| {
                    OpsError::new(
                        CfdErrorCode::CheckIndexOutOfBounds,
                        target.location,
                        format!("数组索引越界: 索引 {idx_us}，长度 {len}"),
                    )
                })
        }
        EvalValue::Dict {
            entries,
            key_type,
            value_type,
        } => {
            let Some(key) = dict_key_from_check_value(&index.value) else {
                return Err(OpsError::eval_type(
                    index.location,
                    format!(
                        "dict 索引不是有效 key: 实际为 {}",
                        format_value_for_message(&index.value)
                    ),
                ));
            };
            let key_label = format_value_for_message(&index.value);
            let value_location = target
                .location
                .clone()
                .map(|location| location.dict_key(key_label.clone()));
            for entry_index in 0..entries.len() {
                let Some((entry_key, _)) = entries.model_entry_at(model, entry_index) else {
                    continue;
                };
                if !dict_key_matches(entry_key, &key) {
                    continue;
                }
                let Some(entry) = entries
                    .located_entry_at(
                        entry_index,
                        key_type.as_ref(),
                        value_type.as_ref(),
                        target.location.as_ref(),
                        model,
                        budget,
                    )
                    .map_err(budget_error)?
                else {
                    break;
                };
                let EvalValue::Entry(entry) = entry.value else {
                    break;
                };
                return Ok(LocatedEvalValue::new(entry.value, value_location));
            }
            Err(OpsError::new(
                CfdErrorCode::CheckMissingDictKey,
                target.location,
                format!("dict key {key_label} 不存在"),
            ))
        }
        other => Err(OpsError::eval_type(
            target.location,
            format!(
                "索引目标不是集合: 读取 [{}] 时实际为 {}",
                format_value_for_message(&index.value),
                format_value_for_message(&other),
            ),
        )),
    }
}

fn budget_error(exceeded: LocatedBudgetExceeded) -> OpsError {
    OpsError::new(
        CfdErrorCode::CheckBudgetExceeded,
        *exceeded.location,
        exceeded.error.to_string(),
    )
}

pub(crate) fn field_type_for_record<'a>(
    schema: &'a CftSchema,
    model: &CfdDataModel,
    record: &EvalRecordRef,
    name: &str,
) -> Option<&'a CftValueType> {
    record
        .actual_type(model)
        .and_then(|actual_type| schema.field(actual_type, name))
        .map(|field| &field.value_type)
}

pub(crate) fn current_field<'model>(
    schema: &CftSchema,
    model: &'model CfdDataModel,
    current: &EvalValue<'model>,
    name: &str,
    budget: &mut StructuralBudget,
) -> OpsResult<Option<LocatedEvalValue<'model>>> {
    let EvalValue::Record(record) = current else {
        return Ok(None);
    };
    if name == "id" {
        return Ok(virtual_id(model, record, record.location()));
    }
    record
        .field(
            model,
            field_type_for_record(schema, model, record, name),
            name,
            budget,
        )
        .map_err(budget_error)
}

pub(crate) fn field_value<'model>(
    schema: &CftSchema,
    model: &'model CfdDataModel,
    target: LocatedEvalValue<'model>,
    name: &str,
    budget: &mut StructuralBudget,
) -> OpsResult<LocatedEvalValue<'model>> {
    if matches!(target.value.scalar(), Some(ScalarValue::Null)) {
        return Err(OpsError::new(
            CfdErrorCode::CheckNullAccess,
            target.location,
            format!("不能访问 null 的字段: 尝试在 null 上读取 `.{name}`"),
        ));
    }
    match target.value {
        EvalValue::Record(record) => {
            if name == "id" {
                return virtual_id(model, &record, record.location())
                    .ok_or_else(|| OpsError::eval_type(None, "记录没有虚拟 id"));
            }
            record
                .field(
                    model,
                    field_type_for_record(schema, model, &record, name),
                    name,
                    budget,
                )
                .map_err(budget_error)?
                .ok_or_else(|| {
                    OpsError::eval_type(target.location, format!("记录没有字段 `{name}`"))
                })
        }
        EvalValue::Entry(entry) => match name {
            "key" => Ok(LocatedEvalValue::new(*entry.key, target.location)),
            "value" => Ok(LocatedEvalValue::new(entry.value, target.location)),
            _ => Err(OpsError::eval_type(
                target.location,
                format!("dict entry 没有字段 `{name}`，只有 `key` 和 `value`"),
            )),
        },
        other => Err(OpsError::eval_type(
            target.location,
            format!(
                "字段访问目标不是对象: 读取 `.{name}` 时实际为 {}",
                format_value_for_message(&other)
            ),
        )),
    }
}

pub(crate) fn virtual_id<'model>(
    model: &CfdDataModel,
    record: &EvalRecordRef,
    location: Option<ValueLocation>,
) -> Option<LocatedEvalValue<'model>> {
    let key = record.key(model).filter(|key| !key.is_empty())?;
    let key = key.to_string();
    let location = location.map(|location| location.field("id"));
    Some(LocatedEvalValue::new(EvalValue::string(key), location))
}
