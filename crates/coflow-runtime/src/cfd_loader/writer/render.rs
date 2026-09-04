use crate::data_model::{CfdDictKey, CfdEnumValue, CfdValue};
use coflow_language::cft::{CftSchema, CftValueType};

use super::schema_nav::{object_type_name, type_after_field_segment};
use super::CFD_INDENT;

/// Serialize a `CfdValue` to CFD source text.
///
/// `depth` controls indentation for nested object bodies. Refs are always
/// emitted as `&key`; the target type is supplied by the surrounding schema
/// context rather than by the value syntax.
#[must_use]
pub(super) fn serialize_value(v: &CfdValue, depth: usize) -> String {
    serialize_value_for_type(v, None, None, depth)
}

pub(super) fn serialize_value_for_type(
    v: &CfdValue,
    schema: Option<&CftSchema>,
    expected: Option<&CftValueType>,
    depth: usize,
) -> String {
    let indent = CFD_INDENT.repeat(depth);
    let outer = CFD_INDENT.repeat(depth.saturating_sub(1));
    match v {
        CfdValue::OptionNone => "None".to_string(),
        CfdValue::OptionSome(value) => {
            serialize_value_for_type(value, schema, option_inner(expected), depth)
        }
        CfdValue::ResultOk(value) => format!(
            "Ok({})",
            serialize_value_for_type(value, schema, result_ok(expected), depth)
        ),
        CfdValue::ResultErr(value) => format!(
            "Err({})",
            serialize_value_for_type(value, schema, result_error(expected), depth)
        ),
        CfdValue::Bool(v) => v.to_string(),
        CfdValue::Int(v) => v.to_string(),
        CfdValue::Float(v) => {
            let s = v.to_string();
            if s.contains('.') || s.contains('e') || s.contains('E') {
                s
            } else {
                format!("{s}.0")
            }
        }
        CfdValue::String(v) => format!("{v:?}"),
        CfdValue::FormattedString(v) => v.source.clone(),
        CfdValue::Function(v) => v.source.clone(),
        CfdValue::Enum(e) => render_enum_value(e, schema, expected),
        CfdValue::Ref(target_key)
            if matches!(
                expected,
                Some(CftValueType::RecordRef(_))
            ) =>
        {
            format!("&{target_key}")
        }
        CfdValue::Ref(target_key) => format!("&{target_key}"),
        CfdValue::Object(boxed) => {
            let body = boxed
                .fields
                .iter()
                .fold(String::new(), |mut acc, (name, value)| {
                    use std::fmt::Write;
                    let field_type = schema
                        .zip(object_type_name(expected, &boxed.actual_type))
                        .and_then(|(schema, type_name)| {
                            type_after_field_segment(schema, type_name, name.as_str())
                        });
                    let _ = writeln!(
                        acc,
                        "{indent}{name}: {},",
                        serialize_value_for_type(value, schema, field_type.as_ref(), depth + 1)
                    );
                    acc
                });
            format!("{} {{\n{body}{outer}}}", boxed.actual_type)
        }
        CfdValue::Array(items) => {
            let item_type = expected.and_then(|ty| match ty {
                CftValueType::Array(inner) => Some(inner.as_ref()),
                _ => None,
            });
            if items.iter().any(requires_multiline_collection_layout) {
                let item_indent = CFD_INDENT.repeat(depth + 1);
                let elems = items
                    .iter()
                    .map(|item| {
                        format!(
                            "{item_indent}{}",
                            serialize_value_for_type(item, schema, item_type, depth + 2)
                        )
                    })
                    .collect::<Vec<_>>();
                return format!("[\n{},\n{indent}]", elems.join(",\n"));
            }
            let elems: Vec<String> = items
                .iter()
                .map(|i| serialize_value_for_type(i, schema, item_type, depth))
                .collect();
            format!("[{}]", elems.join(", "))
        }
        CfdValue::Dict(entries) => {
            let item_type = expected.and_then(|ty| match ty {
                CftValueType::Dict(_, item) => Some(item.as_ref()),
                _ => None,
            });
            let multiline = entries
                .iter()
                .any(|(_, value)| requires_multiline_collection_layout(value));
            let pairs: Vec<String> = entries
                .iter()
                .map(|(k, v)| {
                    let key = match k {
                        CfdDictKey::String(s) => format!("{s:?}"),
                        CfdDictKey::Int(n) => n.to_string(),
                        CfdDictKey::Enum(e) => e.variant.as_ref().map_or_else(
                            || format!("{}({})", e.enum_name, e.value),
                            ToString::to_string,
                        ),
                    };
                    let value_depth = if multiline { depth + 2 } else { depth };
                    format!(
                        "{key}: {}",
                        serialize_value_for_type(v, schema, item_type, value_depth)
                    )
                })
                .collect();
            if multiline {
                let item_indent = CFD_INDENT.repeat(depth + 1);
                let body = pairs
                    .into_iter()
                    .map(|pair| format!("{item_indent}{pair}"))
                    .collect::<Vec<_>>()
                    .join(",\n");
                format!("{{\n{body},\n{indent}}}")
            } else {
                format!("{{{}}}", pairs.join(", "))
            }
        }
    }
}

fn requires_multiline_collection_layout(value: &CfdValue) -> bool {
    match value {
        CfdValue::Object(_) => true,
        CfdValue::OptionSome(inner) | CfdValue::ResultOk(inner) | CfdValue::ResultErr(inner) => {
            requires_multiline_collection_layout(inner)
        }
        CfdValue::Array(items) => items.iter().any(requires_multiline_collection_layout),
        CfdValue::Dict(entries) => entries
            .iter()
            .any(|(_, value)| requires_multiline_collection_layout(value)),
        _ => false,
    }
}

fn render_enum_value(
    value: &CfdEnumValue,
    schema: Option<&CftSchema>,
    expected: Option<&CftValueType>,
) -> String {
    let expected_name = expected.and_then(|ty| match ty {
        CftValueType::Enum(name) => Some(name.as_str()),
        _ => None,
    });
    let schema_enum = schema
        .and_then(|schema| schema.resolve_enum(expected_name.unwrap_or(value.enum_name.as_str())))
        .filter(|schema_enum| schema_enum.name == value.enum_name);
    if let Some(schema_enum) = schema_enum.filter(|schema_enum| schema_enum.is_flag) {
        if value.value == 0 {
            return schema_enum
                .variants
                .iter()
                .find(|variant| variant.value == 0)
                .map_or_else(|| "0".to_string(), |variant| variant.name.to_string());
        }
        let names = schema_enum
            .variants
            .iter()
            .filter(|variant| variant.value != 0 && value.value & variant.value == variant.value)
            .map(|variant| variant.name.to_string())
            .collect::<Vec<_>>();
        let rendered_mask = schema_enum
            .variants
            .iter()
            .filter(|variant| variant.value != 0 && value.value & variant.value == variant.value)
            .fold(0_i64, |mask, variant| mask | variant.value);
        if rendered_mask == value.value && !names.is_empty() {
            return names.join(" | ");
        }
        return value.value.to_string();
    }
    value.variant.as_ref().map_or_else(
        || format!("{}({})", value.enum_name, value.value),
        ToString::to_string,
    )
}

fn option_inner(expected: Option<&CftValueType>) -> Option<&CftValueType> {
    match expected {
        Some(CftValueType::Option(inner)) => Some(inner),
        _ => None,
    }
}

fn result_ok(expected: Option<&CftValueType>) -> Option<&CftValueType> {
    match expected {
        Some(CftValueType::Result(ok, _)) => Some(ok),
        _ => None,
    }
}

fn result_error(expected: Option<&CftValueType>) -> Option<&CftValueType> {
    match expected {
        Some(CftValueType::Result(_, error)) => Some(error),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::serialize_value_for_type;
    use crate::data_model::{CfdEnumValue, CfdObject, CfdValue};
    use coflow_language::cft::{
        build_schema, parse_modules, CftDimensionInputs, CftFile, CftValueType, ModuleId,
    };
    use std::collections::BTreeMap;

    #[test]
    fn serializes_object_collections_with_structural_indentation() -> Result<(), String> {
        let effect = CfdValue::Object(Box::new(
            CfdObject::try_new(
                "HealEffect",
                BTreeMap::from([("amount".to_string(), CfdValue::Int(5))]),
            )
            .map_err(|error| error.to_string())?,
        ));
        let value = CfdValue::Array(vec![effect.clone(), effect]);

        let rendered = serialize_value_for_type(&value, None, None, 1);
        let expected = "[\n    HealEffect {\n      amount: 5,\n    },\n    HealEffect {\n      amount: 5,\n    },\n  ]";
        if rendered != expected {
            return Err(format!("unexpected object array layout:\n{rendered}"));
        }
        Ok(())
    }

    #[test]
    fn serializes_present_options_without_some_constructor() {
        let value = CfdValue::OptionSome(Box::new(CfdValue::Int(7)));
        let ty = CftValueType::Option(Box::new(CftValueType::Int));

        assert_eq!(serialize_value_for_type(&value, None, Some(&ty), 0), "7");
        assert_eq!(
            serialize_value_for_type(&CfdValue::OptionNone, None, Some(&ty), 0),
            "None"
        );
    }

    #[test]
    fn serializes_flag_masks_in_schema_order() -> Result<(), String> {
        let modules = parse_modules([CftFile::from_source(
            ModuleId::from("main"),
            "@flag enum Access { Empty = 0, Read = 1, Write = 2, Execute = 4 }",
        )]);
        let schema = build_schema(&modules, &CftDimensionInputs::default())
            .map_err(|error| format!("{error:?}"))?;
        let schema_enum = schema
            .resolve_enum("Access")
            .ok_or_else(|| "missing Access enum".to_string())?;
        let ty = CftValueType::Enum(schema_enum.name.clone());
        let value = CfdValue::Enum(
            CfdEnumValue::try_new("Access", None::<String>, 5)
                .map_err(|error| error.to_string())?,
        );
        let rendered = serialize_value_for_type(&value, Some(&schema), Some(&ty), 0);
        if rendered != "Read | Execute" {
            return Err(format!(
                "expected flag names in schema order, got `{rendered}`"
            ));
        }

        let zero = CfdValue::Enum(
            CfdEnumValue::try_new("Access", None::<String>, 0)
                .map_err(|error| error.to_string())?,
        );
        let rendered = serialize_value_for_type(&zero, Some(&schema), Some(&ty), 0);
        if rendered != "Empty" {
            return Err(format!("expected the zero flag name, got `{rendered}`"));
        }
        Ok(())
    }
}
