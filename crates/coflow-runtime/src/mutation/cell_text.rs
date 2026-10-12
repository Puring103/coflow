use crate::data_model::cell_value::{parse_cell, render_cell_value, ParsedCell};
use crate::data_model::{CfdPathSegment, CfdValue, LoadedDictKeyDraft, LoadedValueDraft};
use serde_json::{Map, Number, Value};

use crate::{write_rules, ProjectSession};

use super::{coercion::coerce_json_field_value, one_value_error};

pub(crate) fn parse_cell_text_value(
    session: &ProjectSession,
    actual_type: &str,
    path: &[CfdPathSegment],
    text: &str,
) -> Result<CfdValue, crate::api::DiagnosticSet> {
    let expected = write_rules::expected_type_for_cfd_path(
        session.schema(),
        actual_type,
        path,
        "MUTATION-PATH",
        "MUTATION",
    )?;
    let parsed =
        parse_cell(session.schema(), &expected.display_label(), text).map_err(|error| {
            one_value_error(
                error
                    .diagnostics
                    .into_iter()
                    .map(|diagnostic| diagnostic.message)
                    .collect::<Vec<_>>()
                    .join("; "),
            )
        })?;
    let ParsedCell::Value(input) = parsed else {
        return Err(one_value_error(
            "empty cell text omits a value; use `None` for an Option field",
        ));
    };
    validate_explicit_references(session, &input)?;
    // 输入阶段只验证模板并保存源码；写入后统一由模型按实际所属对象求值，避免旧记录值污染预览。
    let json = input_value_to_json(input)?;
    coerce_json_field_value(session, &expected, &json)
}

pub(crate) fn render_cell_text_value(
    value: &CfdValue,
) -> Result<String, crate::api::DiagnosticSet> {
    render_cell_value(value).map_err(|error| one_value_error(error.to_string()))
}

fn validate_explicit_references(
    session: &ProjectSession,
    value: &LoadedValueDraft,
) -> Result<(), crate::api::DiagnosticSet> {
    match value {
        LoadedValueDraft::TypedRecordRef { type_name, key } => {
            if session
                .model()
                .lookup_assignable(session.schema(), type_name, key)
                .is_none()
            {
                return Err(one_value_error(format!("explicit reference `&{type_name}::{key}` was not found or has a different actual type")));
            }
        }
        LoadedValueDraft::OptionSome(inner) => validate_explicit_references(session, inner)?,
        LoadedValueDraft::Array(items) => {
            for item in items {
                validate_explicit_references(session, item)?;
            }
        }
        LoadedValueDraft::Dict(entries) => {
            for (_, item) in entries {
                validate_explicit_references(session, item)?;
            }
        }
        LoadedValueDraft::Object { fields, .. } => {
            for item in fields.values() {
                validate_explicit_references(session, item)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn input_value_to_json(value: LoadedValueDraft) -> Result<Value, crate::api::DiagnosticSet> {
    match value {
        LoadedValueDraft::OptionNone => Ok(tagged_json("$none", Value::Bool(true))),
        LoadedValueDraft::OptionSome(value) => {
            Ok(tagged_json("$some", input_value_to_json(*value)?))
        }
        LoadedValueDraft::Bool(value) => Ok(Value::Bool(value)),
        LoadedValueDraft::Int(value) | LoadedValueDraft::EnumValue { value, .. } => {
            Ok(Value::Number(Number::from(value)))
        }
        LoadedValueDraft::Float(value) => Number::from_f64(value)
            .map(Value::Number)
            .ok_or_else(|| one_value_error("cell float must be finite")),
        LoadedValueDraft::String(value) => Ok(Value::String(value)),
        LoadedValueDraft::FormattedString(formatted) => Ok(Value::String(formatted.source)),

        LoadedValueDraft::EnumVariant { variant, .. } => Ok(Value::String(variant)),
        LoadedValueDraft::TypedRecordRef { type_name: _, key }
        | LoadedValueDraft::RecordRef(key) => {
            let mut object = Map::new();
            object.insert("$ref".to_string(), Value::String(key));
            Ok(Value::Object(object))
        }
        LoadedValueDraft::Object {
            actual_type,
            fields,
        } => {
            let mut object = fields
                .into_iter()
                .map(|(name, value)| Ok((name, input_value_to_json(value)?)))
                .collect::<Result<Map<_, _>, crate::api::DiagnosticSet>>()?;
            if let Some(actual_type) = actual_type {
                object.insert("$type".to_string(), Value::String(actual_type));
            }
            Ok(Value::Object(object))
        }
        LoadedValueDraft::Array(items) => items
            .into_iter()
            .map(input_value_to_json)
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        LoadedValueDraft::Dict(entries) => {
            let entries = entries
                .into_iter()
                .map(|(key, value)| {
                    let mut entry = Map::new();
                    entry.insert("key".to_string(), input_dict_key_to_json(key));
                    entry.insert("value".to_string(), input_value_to_json(value)?);
                    Ok(Value::Object(entry))
                })
                .collect::<Result<Vec<_>, crate::api::DiagnosticSet>>()?;
            let mut object = Map::new();
            object.insert("$dict".to_string(), Value::Array(entries));
            Ok(Value::Object(object))
        }
    }
}

fn tagged_json(tag: &str, value: Value) -> Value {
    let mut object = Map::new();
    object.insert(tag.to_string(), value);
    Value::Object(object)
}

fn input_dict_key_to_json(key: LoadedDictKeyDraft) -> Value {
    match key {
        LoadedDictKeyDraft::String(value) => Value::String(value),
        LoadedDictKeyDraft::Int(value) => Value::Number(Number::from(value)),
        LoadedDictKeyDraft::EnumVariant { variant, .. } => Value::String(variant),
        LoadedDictKeyDraft::EnumValue { value, .. } => Value::Number(Number::from(value)),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::input_value_to_json;
    use crate::data_model::{LoadedDictKeyDraft, LoadedValueDraft};
    use serde_json::json;

    #[test]
    fn converts_nested_cell_input_to_runtime_mutation_json() {
        let value = LoadedValueDraft::object(
            "Stats",
            [
                ("owner", LoadedValueDraft::record_ref("hero")),
                (
                    "labels",
                    LoadedValueDraft::dict([(
                        LoadedDictKeyDraft::Int(2),
                        LoadedValueDraft::String("rare".to_string()),
                    )]),
                ),
            ],
        );

        assert_eq!(
            input_value_to_json(value).expect("convert cell input"),
            json!({
                "$type": "Stats",
                "owner": { "$ref": "hero" },
                "labels": { "$dict": [{ "key": 2, "value": "rare" }] }
            }),
        );
    }
}
