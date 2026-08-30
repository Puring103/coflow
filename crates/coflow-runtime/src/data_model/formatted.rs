use crate::data_model::{
    CfdDataModel, CfdDictKey, CfdFormattedString, CfdValue, LoadedFieldReference,
    LoadedFormatSegment, LoadedFormattedString,
};
use coflow_language::{CftSchema, CftValueType};

/// Resolves all field references in a loaded formatted string.
///
/// # Errors
///
/// Returns an error when a referenced record or field cannot be resolved.
pub fn evaluate_formatted_string(
    schema: &CftSchema,
    model: &CfdDataModel,
    current_type: &str,
    current_key: &str,
    input: &LoadedFormattedString,
) -> Result<CfdFormattedString, String> {
    let mut rendered = String::new();
    for segment in &input.segments {
        match segment {
            LoadedFormatSegment::Text(text) => rendered.push_str(text),
            LoadedFormatSegment::Reference(reference) => {
                rendered.push_str(&stringify_value(resolve_reference(
                    schema,
                    model,
                    current_type,
                    current_key,
                    reference,
                )?));
            }
        }
    }
    Ok(CfdFormattedString {
        source: input.source.clone(),
        rendered,
    })
}

fn resolve_reference<'a>(
    schema: &CftSchema,
    model: &'a CfdDataModel,
    current_type: &str,
    current_key: &str,
    reference: &LoadedFieldReference,
) -> Result<&'a CfdValue, String> {
    let type_name = reference.type_name.as_deref().unwrap_or(current_type);
    let key = reference.key.as_deref().unwrap_or(current_key);
    let record_id = model
        .lookup_assignable(schema, type_name, key)
        .ok_or_else(|| format!("formatted string record `&{type_name}.{key}` was not found"))?;
    let record = model
        .record(record_id)
        .ok_or_else(|| "formatted string record was not found".to_string())?;
    let first = reference
        .path
        .first()
        .ok_or_else(|| "formatted string reference field is missing".to_string())?;
    let mut value = record
        .field(first)
        .ok_or_else(|| format!("unknown field `{first}` on type `{}`", record.actual_type()))?;
    let mut ty = field_type(schema, record.actual_type(), first)?;

    for field in reference.path.iter().skip(1) {
        match (value, &ty) {
            (CfdValue::Object(object), CftValueType::Object(declared_type)) => {
                let actual_type = if object.actual_type().is_empty() {
                    declared_type.as_str()
                } else {
                    object.actual_type()
                };
                value = object
                    .field(field)
                    .ok_or_else(|| format!("unknown field `{field}` on type `{actual_type}`"))?;
                ty = field_type(schema, actual_type, field)?;
            }
            (CfdValue::Ref(key), CftValueType::RecordRef(target_type)) => {
                let target_id = model
                    .lookup_assignable(schema, target_type.as_str(), key.as_str())
                    .ok_or_else(|| {
                        format!("reference target `&{target_type}.{key}` was not found")
                    })?;
                let target = model
                    .record(target_id)
                    .ok_or_else(|| "reference target was not found".to_string())?;
                value = target.field(field).ok_or_else(|| {
                    format!("unknown field `{field}` on type `{}`", target.actual_type())
                })?;
                ty = field_type(schema, target.actual_type(), field)?;
            }
            _ => {
                return Err(format!(
                    "cannot read field `{field}` from formatted string value"
                ))
            }
        }
    }
    Ok(value)
}

fn field_type(schema: &CftSchema, type_name: &str, field: &str) -> Result<CftValueType, String> {
    schema
        .resolve_type(type_name)
        .and_then(|meta| meta.field(field))
        .map(|meta| meta.value_type.clone())
        .ok_or_else(|| format!("unknown field `{field}` on type `{type_name}`"))
}

pub fn stringify_value(value: &CfdValue) -> String {
    match value {
        CfdValue::OptionNone => "None".to_string(),
        CfdValue::OptionSome(value) => format!("Some({})", stringify_value(value)),
        CfdValue::ResultOk(value) => format!("Ok({})", stringify_value(value)),
        CfdValue::ResultErr(value) => format!("Err({})", stringify_value(value)),
        CfdValue::Bool(value) => value.to_string(),
        CfdValue::Int(value) => value.to_string(),
        CfdValue::Float(value) => value.to_string(),
        CfdValue::String(value) => value.clone(),
        CfdValue::FormattedString(value) => value.rendered.clone(),
        CfdValue::Function(value) => value.source.clone(),
        CfdValue::Enum(value) => value
            .variant
            .as_ref()
            .map_or_else(|| value.value.to_string(), ToString::to_string),
        CfdValue::Ref(key) => format!("&{key}"),
        CfdValue::Object(object) => format!(
            "{}{{{}}}",
            object.actual_type(),
            object
                .fields()
                .iter()
                .map(|(name, value)| format!("{name}: {}", nested(value)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        CfdValue::Array(items) => format!(
            "[{}]",
            items.iter().map(nested).collect::<Vec<_>>().join(", ")
        ),
        CfdValue::Dict(entries) => format!(
            "{{{}}}",
            entries
                .iter()
                .map(|(key, value)| format!("{}: {}", dict_key(key), nested(value)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn nested(value: &CfdValue) -> String {
    match value {
        CfdValue::String(value) => format!("{value:?}"),
        CfdValue::FormattedString(value) => format!("{:?}", value.rendered),
        CfdValue::Function(value) => value.source.clone(),
        _ => stringify_value(value),
    }
}

fn dict_key(key: &CfdDictKey) -> String {
    match key {
        CfdDictKey::String(value) => format!("{value:?}"),
        CfdDictKey::Int(value) => value.to_string(),
        CfdDictKey::Enum(value) => value
            .variant
            .as_ref()
            .map_or_else(|| value.value.to_string(), ToString::to_string),
    }
}
