use coflow_data_model::{CfdInputDictKey, CfdInputValue, CfdPathSegment, CfdValue};
use coflow_loader_table_core::cell_value::{parse_cell, render_cell_value, ParsedCell};
use serde_json::{Map, Number, Value};

use crate::{write_rules, ProjectSession};

use super::{coercion::coerce_json_field_value, one_value_error};

pub(crate) fn parse_cell_text_value(
    session: &ProjectSession,
    actual_type: &str,
    path: &[CfdPathSegment],
    text: &str,
) -> Result<CfdValue, coflow_api::DiagnosticSet> {
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
            "empty cell text omits a value; use `null` for a nullable field",
        ));
    };
    let json = input_value_to_json(input)?;
    coerce_json_field_value(session, &expected, &json)
}

pub(crate) fn render_cell_text_value(
    value: &CfdValue,
) -> Result<String, coflow_api::DiagnosticSet> {
    if matches!(value, CfdValue::Null) {
        return Ok("null".to_string());
    }
    render_cell_value(value).map_err(|error| one_value_error(error.to_string()))
}

fn input_value_to_json(value: CfdInputValue) -> Result<Value, coflow_api::DiagnosticSet> {
    match value {
        CfdInputValue::Null => Ok(Value::Null),
        CfdInputValue::Bool(value) => Ok(Value::Bool(value)),
        CfdInputValue::Int(value) => Ok(Value::Number(Number::from(value))),
        CfdInputValue::Float(value) => Number::from_f64(value)
            .map(Value::Number)
            .ok_or_else(|| one_value_error("cell float must be finite")),
        CfdInputValue::String(value) => Ok(Value::String(value)),
        CfdInputValue::EnumVariant { variant, .. } => Ok(Value::String(variant)),
        CfdInputValue::RecordRef(key) => {
            let mut object = Map::new();
            object.insert("$ref".to_string(), Value::String(key));
            Ok(Value::Object(object))
        }
        CfdInputValue::Object {
            actual_type,
            fields,
        } => {
            let mut object = fields
                .into_iter()
                .map(|(name, value)| Ok((name, input_value_to_json(value)?)))
                .collect::<Result<Map<_, _>, coflow_api::DiagnosticSet>>()?;
            if let Some(actual_type) = actual_type {
                object.insert("$type".to_string(), Value::String(actual_type));
            }
            Ok(Value::Object(object))
        }
        CfdInputValue::Array(items) => items
            .into_iter()
            .map(input_value_to_json)
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        CfdInputValue::Dict(entries) => {
            let entries = entries
                .into_iter()
                .map(|(key, value)| {
                    let mut entry = Map::new();
                    entry.insert("key".to_string(), input_dict_key_to_json(key));
                    entry.insert("value".to_string(), input_value_to_json(value)?);
                    Ok(Value::Object(entry))
                })
                .collect::<Result<Vec<_>, coflow_api::DiagnosticSet>>()?;
            let mut object = Map::new();
            object.insert("$dict".to_string(), Value::Array(entries));
            Ok(Value::Object(object))
        }
        CfdInputValue::ObjectSpread { .. } | CfdInputValue::DictSpread { .. } => Err(
            one_value_error("spread cell values cannot be pasted into an effective field value"),
        ),
    }
}

fn input_dict_key_to_json(key: CfdInputDictKey) -> Value {
    match key {
        CfdInputDictKey::String(value) => Value::String(value),
        CfdInputDictKey::Int(value) => Value::Number(Number::from(value)),
        CfdInputDictKey::EnumVariant { variant, .. } => Value::String(variant),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::input_value_to_json;
    use coflow_data_model::{CfdInputDictKey, CfdInputValue};
    use serde_json::json;

    #[test]
    fn converts_nested_cell_input_to_runtime_mutation_json() {
        let value = CfdInputValue::object(
            "Stats",
            [
                ("owner", CfdInputValue::record_ref("hero")),
                (
                    "labels",
                    CfdInputValue::dict([(
                        CfdInputDictKey::Int(2),
                        CfdInputValue::String("rare".to_string()),
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
