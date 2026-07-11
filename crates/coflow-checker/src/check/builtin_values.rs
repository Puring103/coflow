use std::collections::BTreeMap;

use coflow_data_model::CfdErrorCode;
use regex::Regex;

use super::builtins::Builtin;
use super::diagnostics::{format_value_for_message, type_ref_is_float};
use super::ops::{self, OpsError, OpsResult};
use super::value::{
    comparable_key, dict_key_from_check_value, values_equal, CheckValue, LocatedCheckValue,
};

pub(super) fn len_value(value: LocatedCheckValue) -> OpsResult<LocatedCheckValue> {
    match value.value {
        CheckValue::Array { items, .. } => Ok(LocatedCheckValue::new(
            CheckValue::Int(items.len() as i64),
            value.path,
        )),
        CheckValue::Dict { entries, .. } => Ok(LocatedCheckValue::new(
            CheckValue::Int(entries.len() as i64),
            value.path,
        )),
        other => Err(OpsError::eval_type(
            value.path,
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
) -> OpsResult<bool> {
    match &collection.value {
        CheckValue::Array { items, .. } => Ok(items.iter().any(|item| values_equal(item, value))),
        CheckValue::Dict { entries, .. } => {
            let Some(key) = dict_key_from_check_value(value) else {
                return Err(OpsError::eval_type(
                    collection.path.clone(),
                    format!(
                        "contains 的 dict key 无效: 实际为 {}",
                        format_value_for_message(value)
                    ),
                ));
            };
            Ok(entries
                .iter()
                .any(|entry| entry.key_key() == Some(key.clone())))
        }
        other => Err(OpsError::eval_type(
            collection.path.clone(),
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

pub(super) fn unique_value(value: LocatedCheckValue) -> OpsResult<UniqueEvaluation> {
    let items = match value.value {
        CheckValue::Array { items, .. } => items,
        other => {
            return Err(OpsError::eval_type(
                value.path,
                format!(
                    "isUnique 需要 array: 实际为 {}",
                    format_value_for_message(&other)
                ),
            ));
        }
    };
    let mut seen = BTreeMap::new();
    for (index, item) in items.into_iter().enumerate() {
        let Some(key) = comparable_key(&item) else {
            return Err(OpsError::eval_type(
                value.path.clone(),
                format!(
                    "isUnique 元素不可比较: 实际为 {}",
                    format_value_for_message(&item)
                ),
            ));
        };
        if let Some(first_index) = seen.insert(key, index) {
            return Ok(UniqueEvaluation {
                duplicate: Some(format!(
                    "重复值 {} 出现在索引 {first_index} 和 {index}",
                    format_value_for_message(&item)
                )),
                value: LocatedCheckValue::new(CheckValue::Bool(false), value.path),
            });
        }
    }
    Ok(UniqueEvaluation {
        value: LocatedCheckValue::new(CheckValue::Bool(true), value.path),
        duplicate: None,
    })
}

pub(super) fn keys_value(value: LocatedCheckValue) -> OpsResult<LocatedCheckValue> {
    let arg_kind = value.value.clone();
    let CheckValue::Dict {
        entries, key_type, ..
    } = value.value
    else {
        return Err(OpsError::eval_type(
            value.path,
            format!(
                "keys 需要 dict: 实际为 {}",
                format_value_for_message(&arg_kind)
            ),
        ));
    };
    Ok(LocatedCheckValue::new(
        CheckValue::Array {
            items: entries.into_iter().map(|entry| *entry.key).collect(),
            element_type: key_type,
        },
        value.path,
    ))
}

pub(super) fn values_value(value: LocatedCheckValue) -> OpsResult<LocatedCheckValue> {
    let arg_kind = value.value.clone();
    let CheckValue::Dict {
        entries,
        value_type,
        ..
    } = value.value
    else {
        return Err(OpsError::eval_type(
            value.path,
            format!(
                "values 需要 dict: 实际为 {}",
                format_value_for_message(&arg_kind)
            ),
        ));
    };
    Ok(LocatedCheckValue::new(
        CheckValue::Array {
            items: entries.into_iter().map(|entry| entry.value).collect(),
            element_type: value_type,
        },
        value.path,
    ))
}

pub(super) fn matches_value(
    value: LocatedCheckValue,
    pattern: &str,
) -> OpsResult<LocatedCheckValue> {
    let path = value.path.clone();
    let value_kind = value.value.clone();
    let CheckValue::String(text) = value.value else {
        return Err(OpsError::eval_type(
            path,
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
        value.path,
    ))
}

pub(super) fn min_max_value(
    builtin: Builtin,
    value: LocatedCheckValue,
) -> OpsResult<LocatedCheckValue> {
    let path = value.path.clone();
    let arg_kind = value.value.clone();
    let CheckValue::Array { items, .. } = value.value else {
        return Err(OpsError::eval_type(
            path,
            format!(
                "{} 需要 array: 实际为 {}",
                builtin.name(),
                format_value_for_message(&arg_kind)
            ),
        ));
    };
    if items.is_empty() {
        return Err(OpsError::new(
            CfdErrorCode::CheckEmptyMinMax,
            path,
            format!("{} 不能作用于空数组", builtin.name()),
        ));
    }
    let mut non_null_items = items
        .iter()
        .filter(|item| !matches!(item, CheckValue::Null));
    let Some(mut out) = non_null_items.next().cloned() else {
        return Err(OpsError::new(
            CfdErrorCode::CheckEmptyMinMax,
            path,
            format!(
                "{} 不能作用于全 null 数组，长度为 {}",
                builtin.name(),
                items.len()
            ),
        ));
    };
    for item in non_null_items {
        let ord = ops::compare_order(&out, item, path.clone())?;
        if (builtin == Builtin::Min && ord.is_gt()) || (builtin == Builtin::Max && ord.is_lt()) {
            out = item.clone();
        }
    }
    Ok(LocatedCheckValue::new(out, path))
}

pub(super) fn sum_value(value: LocatedCheckValue) -> OpsResult<LocatedCheckValue> {
    let path = value.path.clone();
    let arg_kind = value.value.clone();
    let CheckValue::Array {
        items,
        element_type,
    } = value.value
    else {
        return Err(OpsError::eval_type(
            value.path,
            format!(
                "sum 需要 array: 实际为 {}",
                format_value_for_message(&arg_kind)
            ),
        ));
    };
    let mut int_sum = 0_i64;
    let mut float_sum = 0.0_f64;
    let mut saw_float = false;
    let mut saw_numeric = false;
    for item in items {
        match item {
            CheckValue::Int(value) if !saw_float => {
                saw_numeric = true;
                let Some(next) = int_sum.checked_add(value) else {
                    return Err(OpsError::eval_type(
                        path,
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
                    path,
                    format!(
                        "sum 元素不是数值: 实际为 {}",
                        format_value_for_message(&other)
                    ),
                ));
            }
        }
    }
    if saw_float || (!saw_numeric && type_ref_is_float(element_type.as_ref())) {
        Ok(LocatedCheckValue::new(CheckValue::Float(float_sum), path))
    } else {
        Ok(LocatedCheckValue::new(CheckValue::Int(int_sum), path))
    }
}
