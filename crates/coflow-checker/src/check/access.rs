use coflow_data_model::CfdDataModel;
use coflow_data_model::CfdErrorCode;
use coflow_structure::StructuralBudget;

use super::diagnostics::format_value_for_message;
use super::ops::{OpsError, OpsResult};
use super::value::{
    dict_key_from_check_value, dict_key_matches, CheckValue, LocatedBudgetExceeded,
    LocatedCheckValue,
};

pub(super) fn index_value(
    target: LocatedCheckValue,
    index: LocatedCheckValue,
    model: &CfdDataModel,
    budget: &mut StructuralBudget,
) -> OpsResult<LocatedCheckValue> {
    if matches!(target.value, CheckValue::Null) {
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
        CheckValue::Array {
            items,
            element_type,
        } => {
            let index_location = index.location.clone();
            let index_kind = index.value.clone();
            let CheckValue::Int(idx) = index.value else {
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
        CheckValue::Dict {
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
                let CheckValue::Entry(entry) = entry.value else {
                    break;
                };
                return Ok(LocatedCheckValue::new(entry.value, value_location));
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
        exceeded.location,
        exceeded.error.to_string(),
    )
}
