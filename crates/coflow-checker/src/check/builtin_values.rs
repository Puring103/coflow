use std::collections::BTreeMap;

use coflow_data_model::{CfdDataModel, CfdErrorCode};
use coflow_structure::StructuralBudget;
use regex::Regex;

use super::builtins::Builtin;
use super::diagnostics::{format_value_for_message, value_type_is_float};
use super::ops::{self, OpsError, OpsResult};
use super::value::{
    comparable_key, dict_key_from_check_value, dict_key_matches, values_equal, CheckItems,
    CheckValue, LocatedBudgetExceeded, LocatedCheckValue,
};

pub(super) fn len_value(value: LocatedCheckValue) -> OpsResult<LocatedCheckValue> {
    match value.value {
        CheckValue::Array { items, .. } => Ok(LocatedCheckValue::new(
            CheckValue::Int(items.len() as i64),
            value.location,
        )),
        CheckValue::Dict { entries, .. } => Ok(LocatedCheckValue::new(
            CheckValue::Int(entries.len() as i64),
            value.location,
        )),
        other => Err(OpsError::eval_type(
            value.location,
            format!(
                "len 需要 array 或 dict: 实际为 {}",
                format_value_for_message(&other)
            ),
        )),
    }
}

pub(super) fn contains_value(
    collection: &LocatedCheckValue,
    value: &CheckValue,
    model: &CfdDataModel,
    budget: &mut StructuralBudget,
) -> OpsResult<bool> {
    match &collection.value {
        CheckValue::Array {
            items,
            element_type,
        } => {
            for index in 0..items.len() {
                let Some(item) = items
                    .located_at(
                        index,
                        element_type.as_ref(),
                        collection.location.as_ref(),
                        model,
                        budget,
                    )
                    .map_err(budget_error)?
                else {
                    continue;
                };
                if values_equal(&item.value, value) {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        CheckValue::Dict { entries, .. } => {
            let Some(key) = dict_key_from_check_value(value) else {
                return Err(OpsError::eval_type(
                    collection.location.clone(),
                    format!(
                        "contains 的 dict key 无效: 实际为 {}",
                        format_value_for_message(value)
                    ),
                ));
            };
            Ok((0..entries.len()).any(|index| {
                entries
                    .model_entry_at(model, index)
                    .is_some_and(|(entry_key, _)| dict_key_matches(entry_key, &key))
            }))
        }
        other => Err(OpsError::eval_type(
            collection.location.clone(),
            format!(
                "contains 需要 array 或 dict: 实际为 {}",
                format_value_for_message(other)
            ),
        )),
    }
}

pub(super) struct UniqueEvaluation {
    pub(super) value: LocatedCheckValue,
    pub(super) duplicate: Option<String>,
}

pub(super) fn unique_value(
    value: LocatedCheckValue,
    model: &CfdDataModel,
    budget: &mut StructuralBudget,
) -> OpsResult<UniqueEvaluation> {
    let CheckValue::Array {
        items,
        element_type,
    } = &value.value
    else {
        return Err(OpsError::eval_type(
            value.location,
            format!(
                "isUnique 需要 array: 实际为 {}",
                format_value_for_message(&value.value)
            ),
        ));
    };
    let mut seen = BTreeMap::new();
    for index in 0..items.len() {
        let Some(item) = items
            .located_at(
                index,
                element_type.as_ref(),
                value.location.as_ref(),
                model,
                budget,
            )
            .map_err(budget_error)?
        else {
            continue;
        };
        let Some(key) = comparable_key(&item.value) else {
            return Err(OpsError::eval_type(
                value.location.clone(),
                format!(
                    "isUnique 元素不可比较: 实际为 {}",
                    format_value_for_message(&item.value)
                ),
            ));
        };
        if let Some(first_index) = seen.insert(key, index) {
            return Ok(UniqueEvaluation {
                duplicate: Some(format!(
                    "重复值 {} 出现在索引 {first_index} 和 {index}",
                    format_value_for_message(&item.value)
                )),
                value: LocatedCheckValue::new(CheckValue::Bool(false), value.location),
            });
        }
    }
    Ok(UniqueEvaluation {
        value: LocatedCheckValue::new(CheckValue::Bool(true), value.location),
        duplicate: None,
    })
}

pub(super) fn keys_value(value: LocatedCheckValue) -> OpsResult<LocatedCheckValue> {
    let CheckValue::Dict {
        entries, key_type, ..
    } = value.value
    else {
        return Err(OpsError::eval_type(value.location, "keys 需要 dict"));
    };
    Ok(LocatedCheckValue::new(
        CheckValue::Array {
            items: CheckItems::DictKeys(entries),
            element_type: key_type,
        },
        value.location,
    ))
}

pub(super) fn values_value(value: LocatedCheckValue) -> OpsResult<LocatedCheckValue> {
    let CheckValue::Dict {
        entries,
        value_type,
        ..
    } = value.value
    else {
        return Err(OpsError::eval_type(value.location, "values 需要 dict"));
    };
    Ok(LocatedCheckValue::new(
        CheckValue::Array {
            items: CheckItems::DictValues(entries),
            element_type: value_type,
        },
        value.location,
    ))
}

pub(super) fn matches_value(
    value: LocatedCheckValue,
    pattern: &str,
) -> OpsResult<LocatedCheckValue> {
    let location = value.location.clone();
    let value_kind = value.value.clone();
    let CheckValue::String(text) = value.value else {
        return Err(OpsError::eval_type(
            location,
            format!(
                "matches 的值不是 string: 实际为 {}",
                format_value_for_message(&value_kind)
            ),
        ));
    };
    let regex = Regex::new(pattern).map_err(|err| {
        OpsError::eval_type(None, format!("正则 pattern `{pattern}` 无法编译: {err}"))
    })?;
    Ok(LocatedCheckValue::new(
        CheckValue::Bool(regex.is_match(&text)),
        value.location,
    ))
}

pub(super) fn min_max_value(
    builtin: Builtin,
    value: LocatedCheckValue,
    model: &CfdDataModel,
    budget: &mut StructuralBudget,
) -> OpsResult<LocatedCheckValue> {
    let location = value.location.clone();
    let CheckValue::Array {
        items,
        element_type,
    } = &value.value
    else {
        return Err(OpsError::eval_type(
            location,
            format!(
                "{} 需要 array: 实际为 {}",
                builtin.name(),
                format_value_for_message(&value.value)
            ),
        ));
    };
    if items.len() == 0 {
        return Err(OpsError::new(
            CfdErrorCode::CheckEmptyMinMax,
            location,
            format!("{} 不能作用于空数组", builtin.name()),
        ));
    }
    let mut out = None;
    for index in 0..items.len() {
        let Some(item) = items
            .located_at(
                index,
                element_type.as_ref(),
                value.location.as_ref(),
                model,
                budget,
            )
            .map_err(budget_error)?
        else {
            continue;
        };
        if matches!(item.value, CheckValue::Null) {
            continue;
        }
        match &mut out {
            None => out = Some(item.value),
            Some(current) => {
                let ord = ops::compare_order(current, &item.value, location.clone())?;
                if (builtin == Builtin::Min && ord.is_gt())
                    || (builtin == Builtin::Max && ord.is_lt())
                {
                    *current = item.value;
                }
            }
        }
    }
    let Some(out) = out else {
        return Err(OpsError::new(
            CfdErrorCode::CheckEmptyMinMax,
            location,
            format!(
                "{} 不能作用于全 null 数组，长度为 {}",
                builtin.name(),
                items.len()
            ),
        ));
    };
    Ok(LocatedCheckValue::new(out, location))
}

pub(super) fn sum_value(
    value: LocatedCheckValue,
    model: &CfdDataModel,
    budget: &mut StructuralBudget,
) -> OpsResult<LocatedCheckValue> {
    let location = value.location.clone();
    let CheckValue::Array {
        items,
        element_type,
    } = &value.value
    else {
        return Err(OpsError::eval_type(
            value.location,
            format!(
                "sum 需要 array: 实际为 {}",
                format_value_for_message(&value.value)
            ),
        ));
    };
    let mut int_sum = 0_i64;
    let mut float_sum = 0.0_f64;
    let mut saw_float = false;
    let mut saw_numeric = false;
    for index in 0..items.len() {
        let Some(item) = items
            .located_at(
                index,
                element_type.as_ref(),
                value.location.as_ref(),
                model,
                budget,
            )
            .map_err(budget_error)?
        else {
            continue;
        };
        match item.value {
            CheckValue::Int(value) if !saw_float => {
                saw_numeric = true;
                let Some(next) = int_sum.checked_add(value) else {
                    return Err(OpsError::eval_type(
                        location,
                        format!("整数求和溢出: {int_sum} + {value}"),
                    ));
                };
                int_sum = next;
            }
            CheckValue::Int(value) => {
                saw_numeric = true;
                float_sum += value as f64;
            }
            CheckValue::Float(value) => {
                saw_numeric = true;
                if !saw_float {
                    saw_float = true;
                    float_sum = int_sum as f64;
                }
                float_sum += value;
            }
            CheckValue::Null => {}
            other => {
                return Err(OpsError::eval_type(
                    location,
                    format!(
                        "sum 元素不是数值: 实际为 {}",
                        format_value_for_message(&other)
                    ),
                ));
            }
        }
    }
    if saw_float || (!saw_numeric && value_type_is_float(element_type.as_ref())) {
        Ok(LocatedCheckValue::new(
            CheckValue::Float(float_sum),
            location,
        ))
    } else {
        Ok(LocatedCheckValue::new(CheckValue::Int(int_sum), location))
    }
}

fn budget_error(exceeded: LocatedBudgetExceeded) -> OpsError {
    OpsError::new(
        CfdErrorCode::CheckBudgetExceeded,
        *exceeded.location,
        exceeded.error.to_string(),
    )
}
