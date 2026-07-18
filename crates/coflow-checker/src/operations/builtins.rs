pub(crate) use coflow_cft::CftCheckBuiltin as Builtin;

use coflow_cft::{CftSchemaCheckExpr, CftSchemaCheckExprKind};

pub(crate) enum CallTarget {
    EnumConstructor,
    Builtin(Builtin),
}

pub(crate) struct CallSignature {
    pub(crate) target: CallTarget,
}

impl CallSignature {
    pub(crate) fn resolve_function(
        name: &str,
        arg_count: usize,
        is_enum_name: bool,
    ) -> Result<Self, CallSignatureError> {
        if is_enum_name {
            if arg_count == 1 {
                return Ok(Self {
                    target: CallTarget::EnumConstructor,
                });
            }
            return Err(CallSignatureError::Arity {
                message: "枚举构造函数需要 1 个参数".to_string(),
            });
        }

        let Some(builtin) = Builtin::by_name(name) else {
            return Err(CallSignatureError::UnknownFunction {
                name: name.to_string(),
            });
        };
        require_arity(builtin, arg_count, builtin.arity())?;
        Ok(Self {
            target: CallTarget::Builtin(builtin),
        })
    }

    pub(crate) fn resolve_method(name: &str, arg_count: usize) -> Result<Self, CallSignatureError> {
        let Some(builtin) = Builtin::by_name(name) else {
            return Err(CallSignatureError::UnknownFunction {
                name: name.to_string(),
            });
        };
        let expected_args = builtin.method_arity();
        require_arity(builtin, arg_count, expected_args)?;
        Ok(Self {
            target: CallTarget::Builtin(builtin),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CallSignatureError {
    UnknownFunction { name: String },
    Arity { message: String },
}

pub(crate) fn matches_pattern_arg(arg: &CftSchemaCheckExpr) -> Result<&str, CallSignatureError> {
    let CftSchemaCheckExprKind::String(pattern) = &arg.kind else {
        return Err(CallSignatureError::Arity {
            message: "matches 的 pattern 必须是字符串字面量".to_string(),
        });
    };
    Ok(pattern)
}

fn require_arity(
    builtin: Builtin,
    actual: usize,
    expected: usize,
) -> Result<(), CallSignatureError> {
    if actual == expected {
        return Ok(());
    }
    Err(CallSignatureError::Arity {
        message: format!("{} 需要 {} 个参数", builtin.name(), expected),
    })
}

use coflow_cft::{CftSchema, EnumName};
use coflow_data_model::CfdEnumValue;

pub(crate) fn enum_with_value(
    schema: &CftSchema,
    enum_name: &EnumName,
    value: i64,
) -> CfdEnumValue {
    match schema.enum_value_from_int(enum_name.as_str(), value) {
        Some(enum_value) => enum_value.into(),
        None => anonymous_enum_value(enum_name, value),
    }
}

pub(crate) fn anonymous_enum_value(enum_name: &EnumName, value: i64) -> CfdEnumValue {
    CfdEnumValue {
        enum_name: enum_name.clone(),
        variant: None,
        value,
    }
}

use std::collections::BTreeMap;

use coflow_data_model::{CfdDataModel, CfdErrorCode};
use coflow_structure::StructuralBudget;
use regex::Regex;

use super::diagnostics::{format_value_for_message, value_type_is_float};
use super::ops::{self, OpsError, OpsResult};
use super::value::{
    comparable_key, dict_key_from_check_value, dict_key_matches, values_equal, EvalItems,
    EvalValue, LocatedBudgetExceeded, LocatedEvalValue, ScalarValue,
};

pub(crate) type RegexCache = BTreeMap<String, Regex>;

pub(crate) fn len_value(value: LocatedEvalValue<'_>) -> OpsResult<LocatedEvalValue<'_>> {
    match value.value {
        EvalValue::Array { items, .. } => Ok(LocatedEvalValue::new(
            EvalValue::int(items.len() as i64),
            value.location,
        )),
        EvalValue::Dict { entries, .. } => Ok(LocatedEvalValue::new(
            EvalValue::int(entries.len() as i64),
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

pub(crate) fn contains_value(
    collection: &LocatedEvalValue<'_>,
    value: &EvalValue<'_>,
    model: &CfdDataModel,
    budget: &mut StructuralBudget,
) -> OpsResult<bool> {
    match &collection.value {
        EvalValue::Array {
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
        EvalValue::Dict { entries, .. } => {
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

pub(crate) struct UniqueEvaluation<'model> {
    pub(crate) value: LocatedEvalValue<'model>,
    pub(crate) duplicate: Option<String>,
}

pub(crate) fn unique_value<'model>(
    value: LocatedEvalValue<'model>,
    model: &'model CfdDataModel,
    budget: &mut StructuralBudget,
) -> OpsResult<UniqueEvaluation<'model>> {
    let EvalValue::Array {
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
                value: LocatedEvalValue::new(EvalValue::bool(false), value.location),
            });
        }
    }
    Ok(UniqueEvaluation {
        value: LocatedEvalValue::new(EvalValue::bool(true), value.location),
        duplicate: None,
    })
}

pub(crate) fn keys_value(value: LocatedEvalValue<'_>) -> OpsResult<LocatedEvalValue<'_>> {
    let EvalValue::Dict {
        entries, key_type, ..
    } = value.value
    else {
        return Err(OpsError::eval_type(value.location, "keys 需要 dict"));
    };
    Ok(LocatedEvalValue::new(
        EvalValue::Array {
            items: EvalItems::DictKeys(entries),
            element_type: key_type,
        },
        value.location,
    ))
}

pub(crate) fn values_value(value: LocatedEvalValue<'_>) -> OpsResult<LocatedEvalValue<'_>> {
    let EvalValue::Dict {
        entries,
        value_type,
        ..
    } = value.value
    else {
        return Err(OpsError::eval_type(value.location, "values 需要 dict"));
    };
    Ok(LocatedEvalValue::new(
        EvalValue::Array {
            items: EvalItems::DictValues(entries),
            element_type: value_type,
        },
        value.location,
    ))
}

pub(crate) fn matches_value<'model>(
    value: LocatedEvalValue<'model>,
    pattern: &str,
    cache: &mut RegexCache,
) -> OpsResult<LocatedEvalValue<'model>> {
    let location = value.location.clone();
    let value_kind = value.value.clone();
    let Some(ScalarValue::String(text)) = value.value.scalar() else {
        return Err(OpsError::eval_type(
            location,
            format!(
                "matches 的值不是 string: 实际为 {}",
                format_value_for_message(&value_kind)
            ),
        ));
    };
    if !cache.contains_key(pattern) {
        let regex = Regex::new(pattern).map_err(|err| {
            OpsError::eval_type(None, format!("正则 pattern `{pattern}` 无法编译: {err}"))
        })?;
        cache.insert(pattern.to_string(), regex);
    }
    let matches = cache.get(pattern).is_some_and(|regex| regex.is_match(text));
    Ok(LocatedEvalValue::new(
        EvalValue::bool(matches),
        value.location,
    ))
}

pub(crate) fn min_max_value<'model>(
    builtin: Builtin,
    value: &LocatedEvalValue<'model>,
    model: &'model CfdDataModel,
    budget: &mut StructuralBudget,
) -> OpsResult<LocatedEvalValue<'model>> {
    let location = value.location.clone();
    let EvalValue::Array {
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
        if matches!(item.value.scalar(), Some(ScalarValue::Null)) {
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
    Ok(LocatedEvalValue::new(out, location))
}

pub(crate) fn sum_value<'model>(
    value: LocatedEvalValue<'model>,
    model: &'model CfdDataModel,
    budget: &mut StructuralBudget,
) -> OpsResult<LocatedEvalValue<'model>> {
    let location = value.location.clone();
    let EvalValue::Array {
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
        match item.value.scalar() {
            Some(ScalarValue::Int(value)) if !saw_float => {
                saw_numeric = true;
                let Some(next) = int_sum.checked_add(value) else {
                    return Err(OpsError::eval_type(
                        location,
                        format!("整数求和溢出: {int_sum} + {value}"),
                    ));
                };
                int_sum = next;
            }
            Some(ScalarValue::Int(value)) => {
                saw_numeric = true;
                float_sum += value as f64;
            }
            Some(ScalarValue::Float(value)) => {
                saw_numeric = true;
                if !saw_float {
                    saw_float = true;
                    float_sum = int_sum as f64;
                }
                float_sum += value;
            }
            Some(ScalarValue::Null) => {}
            _ => {
                return Err(OpsError::eval_type(
                    location,
                    format!(
                        "sum 元素不是数值: 实际为 {}",
                        format_value_for_message(&item.value)
                    ),
                ));
            }
        }
    }
    if saw_float || (!saw_numeric && value_type_is_float(element_type.as_ref())) {
        Ok(LocatedEvalValue::new(EvalValue::float(float_sum), location))
    } else {
        Ok(LocatedEvalValue::new(EvalValue::int(int_sum), location))
    }
}

fn budget_error(exceeded: LocatedBudgetExceeded) -> OpsError {
    OpsError::new(
        CfdErrorCode::CheckBudgetExceeded,
        *exceeded.location,
        exceeded.error.to_string(),
    )
}

#[cfg(test)]
mod regex_cache_tests {
    #![allow(clippy::expect_used)]

    use super::*;

    #[test]
    fn repeated_matches_reuse_one_compiled_regex() {
        let mut cache = RegexCache::new();
        for _ in 0..2 {
            let value = LocatedEvalValue::new(EvalValue::string("item_42"), None);
            let matched = matches_value(value, r"^item_\d+$", &mut cache)
                .expect("validated regex should execute");
            assert!(matches!(
                matched.value.scalar(),
                Some(ScalarValue::Bool(true))
            ));
        }
        assert_eq!(cache.len(), 1);
    }
}
