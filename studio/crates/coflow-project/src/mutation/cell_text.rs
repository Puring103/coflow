use crate::cell_value::{parse_schema_cell, ParsedCell};
use crate::data_model::{CfdPathSegment, CfdValue, LoadedDictKeyDraft, LoadedValueDraft};
use serde_json::{Map, Number, Value};

use crate::{write_rules, ProjectSession};

use super::{coercion::coerce_json_field_value, one_value_error};

pub(crate) fn parse_cell_text_value(
    session: &ProjectSession,
    actual_type: &str,
    _key: &str,
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
        parse_schema_cell(session.schema(), &expected, text).map_err(|error| {
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
            "empty cell text omits a value; use `null` for a nullable field",
        ));
    };
    let input = match input {
        LoadedValueDraft::FormattedString(formatted) => {
            return Ok(CfdValue::FormattedString(crate::CallableSource {
                from_default: false,
                location: None,
                imports: Default::default(),
                constant_origin: formatted.constant_origin,
                source: formatted.source,
            }));
        }
        other => other,
    };
    let json = input_value_to_json(input)?;
    coerce_json_field_value(session, &expected, &json)
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
            .or_else(|| {
                coflow_core::serde_float::special(value).map(|text| Value::String(text.into()))
            })
            .ok_or_else(|| one_value_error("invalid float")),
        LoadedValueDraft::String(value) => Ok(Value::String(value)),
        LoadedValueDraft::FormattedString(_) => Err(one_value_error(
            "formatted strings must be evaluated before JSON coercion",
        )),
        LoadedValueDraft::Function(_) => Err(one_value_error(
            "function values cannot be coerced through JSON mutation input",
        )),
        LoadedValueDraft::EnumVariant { variant, .. } => Ok(Value::String(variant)),
        LoadedValueDraft::RecordRef(key) => {
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
        LoadedDictKeyDraft::Bool(value) => Value::Bool(value),
        LoadedDictKeyDraft::EnumVariant { variant, .. } => Value::String(variant),
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
