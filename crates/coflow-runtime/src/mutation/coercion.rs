use std::collections::BTreeMap;

use coflow_api::DiagnosticSet;
use coflow_cft::CftSchemaTypeRef;
use coflow_data_model::{CfdDictKey, CfdEnumValue, CfdObject, CfdValue};
use serde_json::{Map, Value};

use crate::write_rules;
use crate::ProjectSession;

use super::{enum_value, one_value_error, schema_field, MutationValue};

pub(super) fn coerce_mutation_value(
    session: &ProjectSession,
    expected: &CftSchemaTypeRef,
    value: MutationValue,
    pending_records: &BTreeMap<crate::RecordCoordinate, usize>,
) -> Result<CfdValue, DiagnosticSet> {
    let value = match value {
        MutationValue::Json(value) => coerce_json_value(session, expected, &value),
        MutationValue::Cfd(value) => coerce_cfd_value(session, expected, value),
    }?;
    validate_value_for_write(session, expected, &value, pending_records)?;
    Ok(value)
}

pub(super) fn coerce_json_field_value(
    session: &ProjectSession,
    field_ty: &CftSchemaTypeRef,
    value: &Value,
) -> Result<CfdValue, DiagnosticSet> {
    coerce_json_value(session, field_ty, value)
}

pub(super) fn coerce_cfd_field_value(
    session: &ProjectSession,
    field_ty: &CftSchemaTypeRef,
    value: CfdValue,
) -> Result<CfdValue, DiagnosticSet> {
    coerce_cfd_value(session, field_ty, value)
}

fn coerce_json_value(
    session: &ProjectSession,
    expected: &CftSchemaTypeRef,
    value: &Value,
) -> Result<CfdValue, DiagnosticSet> {
    match expected {
        CftSchemaTypeRef::Int => value
            .as_i64()
            .map(CfdValue::Int)
            .ok_or_else(|| one_value_error("expected int")),
        CftSchemaTypeRef::Float => value
            .as_f64()
            .map(CfdValue::Float)
            .ok_or_else(|| one_value_error("expected float")),
        CftSchemaTypeRef::Bool => value
            .as_bool()
            .map(CfdValue::Bool)
            .ok_or_else(|| one_value_error("expected bool")),
        CftSchemaTypeRef::String => value
            .as_str()
            .map(|text| CfdValue::String(text.to_string()))
            .ok_or_else(|| one_value_error("expected string")),
        CftSchemaTypeRef::Nullable(_) if value.is_null() => Ok(CfdValue::Null),
        CftSchemaTypeRef::Nullable(inner) => coerce_json_value(session, inner, value),
        CftSchemaTypeRef::Array(inner) => {
            let items = value
                .as_array()
                .ok_or_else(|| one_value_error("expected array"))?;
            items
                .iter()
                .map(|item| coerce_json_value(session, inner, item))
                .collect::<Result<Vec<_>, _>>()
                .map(CfdValue::Array)
        }
        CftSchemaTypeRef::Dict(key, item) => coerce_json_dict_value(session, key, item, value),
        CftSchemaTypeRef::RecordRef(target_type) => json_ref_key(value)
            .map(|key| CfdValue::Ref(key.to_string()))
            .ok_or_else(|| one_value_error(format!("expected record key for `&{target_type}`"))),
        CftSchemaTypeRef::Enum(name) => {
            let variant = value
                .as_str()
                .ok_or_else(|| one_value_error(format!("expected enum variant for `{name}`")))?;
            enum_value(session, name, variant).map(CfdValue::Enum)
        }
        CftSchemaTypeRef::Object(name) => coerce_json_named_value(session, name, value),
    }
}

fn json_ref_key(value: &Value) -> Option<&str> {
    if let Some(key) = value.as_str() {
        return Some(key);
    }
    let object = value.as_object()?;
    if object.len() != 1 {
        return None;
    }
    object.get("$ref")?.as_str()
}

fn coerce_cfd_value(
    session: &ProjectSession,
    expected: &CftSchemaTypeRef,
    value: CfdValue,
) -> Result<CfdValue, DiagnosticSet> {
    if let CftSchemaTypeRef::Nullable(inner) = expected {
        return if matches!(value, CfdValue::Null) {
            Ok(CfdValue::Null)
        } else {
            coerce_cfd_value(session, inner, value)
        };
    }
    match (expected, value) {
        (CftSchemaTypeRef::Int, value @ CfdValue::Int(_))
        | (CftSchemaTypeRef::Float, value @ CfdValue::Float(_))
        | (CftSchemaTypeRef::Bool, value @ CfdValue::Bool(_))
        | (CftSchemaTypeRef::String, value @ CfdValue::String(_)) => Ok(value),
        (CftSchemaTypeRef::Array(inner), CfdValue::Array(items)) => items
            .into_iter()
            .map(|item| coerce_cfd_value(session, inner, item))
            .collect::<Result<Vec<_>, DiagnosticSet>>()
            .map(CfdValue::Array),
        (CftSchemaTypeRef::Dict(key_type, item_type), CfdValue::Dict(entries)) => entries
            .into_iter()
            .map(|(key, item)| {
                Ok((
                    coerce_cfd_dict_key(session, key_type, key)?,
                    coerce_cfd_value(session, item_type, item)?,
                ))
            })
            .collect::<Result<Vec<_>, DiagnosticSet>>()
            .map(CfdValue::Dict),
        (CftSchemaTypeRef::Enum(name), CfdValue::Enum(enum_value)) => {
            coerce_cfd_enum_value(session, name, enum_value).map(CfdValue::Enum)
        }
        (CftSchemaTypeRef::Object(expected_type), CfdValue::Object(record)) => {
            ensure_object_type_assignable(session, expected_type, record.actual_type())?;
            let mut record = *record;
            let actual_type = record.actual_type().to_string();
            record.fields = coerce_cfd_object_fields(
                session,
                &actual_type,
                std::mem::take(&mut record.fields),
            )?;
            Ok(CfdValue::Object(Box::new(record)))
        }
        (CftSchemaTypeRef::RecordRef(_expected_type), CfdValue::Ref(target_key)) => {
            if target_key.is_empty() {
                return Err(one_value_error("reference key must not be empty"));
            }
            Ok(CfdValue::Ref(target_key))
        }
        (CftSchemaTypeRef::Object(_), CfdValue::Ref(_)) => Err(one_value_error(
            "inline object fields do not accept record refs",
        )),
        _ => Err(one_value_error("value does not match expected schema type")),
    }
}

fn coerce_cfd_object_fields(
    session: &ProjectSession,
    actual_type: &str,
    fields: BTreeMap<String, CfdValue>,
) -> Result<BTreeMap<String, CfdValue>, DiagnosticSet> {
    let schema = session.schema();
    fields
        .into_iter()
        .map(|(name, value)| {
            let field = schema_field(schema, actual_type, &name)?;
            Ok((name, coerce_cfd_field_value(session, &field.ty_ref, value)?))
        })
        .collect()
}

fn validate_value_for_write(
    session: &ProjectSession,
    expected: &CftSchemaTypeRef,
    value: &CfdValue,
    pending_records: &BTreeMap<crate::RecordCoordinate, usize>,
) -> Result<(), DiagnosticSet> {
    let schema = session.schema();
    write_rules::validate_value_for_write_with_pending(
        session,
        schema,
        expected,
        value,
        pending_records,
        "MUTATION-SHAPE",
        "MUTATION",
    )
}

fn coerce_cfd_dict_key(
    session: &ProjectSession,
    key_type: &CftSchemaTypeRef,
    key: CfdDictKey,
) -> Result<CfdDictKey, DiagnosticSet> {
    match (key_type, key) {
        (CftSchemaTypeRef::Nullable(inner), key) => coerce_cfd_dict_key(session, inner, key),
        (CftSchemaTypeRef::String, key @ CfdDictKey::String(_))
        | (CftSchemaTypeRef::Int, key @ CfdDictKey::Int(_)) => Ok(key),
        (CftSchemaTypeRef::Enum(enum_name), CfdDictKey::Enum(value)) => {
            coerce_cfd_enum_value(session, enum_name, value).map(CfdDictKey::Enum)
        }
        _ => Err(one_value_error(
            "dict key does not match expected schema type",
        )),
    }
}

fn coerce_cfd_enum_value(
    session: &ProjectSession,
    enum_name: &str,
    mut value: CfdEnumValue,
) -> Result<CfdEnumValue, DiagnosticSet> {
    if value.enum_name != enum_name {
        return Err(one_value_error(format!(
            "expected enum `{enum_name}`, got `{}`",
            value.enum_name
        )));
    }
    if let Some(variant) = value.variant.as_ref() {
        // The variant name is authoritative; the backing int on the wire may
        // be stale if the editor reuses a previous selection.
        let schema = session.schema();
        let expected_value = schema
            .enum_variant_value(enum_name, variant)
            .ok_or_else(|| {
                one_value_error(format!("unknown enum variant `{enum_name}.{variant}`"))
            })?;
        value.value = expected_value;
    }
    Ok(value)
}

fn coerce_json_dict_value(
    session: &ProjectSession,
    key_type: &CftSchemaTypeRef,
    item_type: &CftSchemaTypeRef,
    value: &Value,
) -> Result<CfdValue, DiagnosticSet> {
    let Some(object) = value.as_object() else {
        return Err(one_value_error("expected dict object"));
    };
    if let Some(entries) = object.get("$dict") {
        if object.len() != 1 {
            return Err(one_value_error("`$dict` object cannot include other keys"));
        }
        return coerce_json_special_dict(session, key_type, item_type, entries);
    }

    object
        .iter()
        .map(|(key, entry_value)| {
            let key_value = Value::String(key.clone());
            Ok((
                coerce_dict_key(session, key_type, &key_value)?,
                coerce_json_value(session, item_type, entry_value)?,
            ))
        })
        .collect::<Result<Vec<_>, DiagnosticSet>>()
        .map(CfdValue::Dict)
}

fn coerce_json_special_dict(
    session: &ProjectSession,
    key_type: &CftSchemaTypeRef,
    item_type: &CftSchemaTypeRef,
    entries: &Value,
) -> Result<CfdValue, DiagnosticSet> {
    let entries = entries
        .as_array()
        .ok_or_else(|| one_value_error("`$dict` must be an array"))?;
    entries
        .iter()
        .map(|entry| {
            let object = entry
                .as_object()
                .ok_or_else(|| one_value_error("`$dict` entries must be objects"))?;
            let key = object
                .get("key")
                .ok_or_else(|| one_value_error("`$dict` entry is missing `key`"))?;
            let value = object
                .get("value")
                .ok_or_else(|| one_value_error("`$dict` entry is missing `value`"))?;
            Ok((
                coerce_dict_key(session, key_type, key)?,
                coerce_json_value(session, item_type, value)?,
            ))
        })
        .collect::<Result<Vec<_>, DiagnosticSet>>()
        .map(CfdValue::Dict)
}

fn coerce_dict_key(
    session: &ProjectSession,
    key_type: &CftSchemaTypeRef,
    value: &Value,
) -> Result<CfdDictKey, DiagnosticSet> {
    match key_type {
        CftSchemaTypeRef::String => value
            .as_str()
            .map(|text| CfdDictKey::String(text.to_string()))
            .ok_or_else(|| one_value_error("expected string dict key")),
        CftSchemaTypeRef::Int => coerce_int_dict_key(value),
        CftSchemaTypeRef::Enum(enum_name) => {
            let variant = value.as_str().ok_or_else(|| {
                one_value_error(format!("expected enum dict key for `{enum_name}`"))
            })?;
            enum_value(session, enum_name, variant).map(CfdDictKey::Enum)
        }
        CftSchemaTypeRef::Nullable(inner) => coerce_dict_key(session, inner, value),
        _ => Err(one_value_error(
            "dict keys support only string, int, and enum types",
        )),
    }
}

fn coerce_int_dict_key(value: &Value) -> Result<CfdDictKey, DiagnosticSet> {
    if let Some(number) = value.as_i64() {
        return Ok(CfdDictKey::Int(number));
    }
    let text = value
        .as_str()
        .ok_or_else(|| one_value_error("expected int dict key"))?;
    let number = text
        .parse::<i64>()
        .map_err(|_| one_value_error(format!("expected int dict key, got `{text}`")))?;
    Ok(CfdDictKey::Int(number))
}

fn coerce_json_named_value(
    session: &ProjectSession,
    expected_type: &str,
    value: &Value,
) -> Result<CfdValue, DiagnosticSet> {
    let object = value
        .as_object()
        .ok_or_else(|| one_value_error(format!("expected object for `{expected_type}`")))?;
    let actual_type = actual_object_type(object, expected_type)?;
    ensure_object_type_assignable(session, expected_type, &actual_type)?;
    let fields = coerce_json_object_fields(session, &actual_type, object)?;
    Ok(CfdValue::Object(Box::new(CfdObject::new(
        actual_type,
        fields,
    ))))
}

fn actual_object_type(
    object: &Map<String, Value>,
    expected_type: &str,
) -> Result<String, DiagnosticSet> {
    object.get("$type").map_or_else(
        || Ok(expected_type.to_string()),
        |value| {
            value
                .as_str()
                .map(ToOwned::to_owned)
                .ok_or_else(|| one_value_error("`$type` must be a string"))
        },
    )
}

fn ensure_object_type_assignable(
    session: &ProjectSession,
    expected_type: &str,
    actual_type: &str,
) -> Result<(), DiagnosticSet> {
    write_rules::ensure_object_type_assignable(
        session.schema(),
        expected_type,
        actual_type,
        "MUTATION-VALUE",
        "MUTATION",
    )
}

fn coerce_json_object_fields(
    session: &ProjectSession,
    actual_type: &str,
    object: &Map<String, Value>,
) -> Result<BTreeMap<String, CfdValue>, DiagnosticSet> {
    let schema = session.schema();
    if schema.resolve_type(actual_type).is_none() {
        return Err(one_value_error(format!(
            "unknown object type `{actual_type}`"
        )));
    }
    let mut fields = BTreeMap::new();
    for (field_name, field_value) in object {
        if field_name == "$type" {
            continue;
        }
        if field_name.starts_with('$') {
            return Err(one_value_error(format!(
                "unsupported object form key `{field_name}`"
            )));
        }
        let field = schema_field(schema, actual_type, field_name)?;
        fields.insert(
            field_name.clone(),
            coerce_json_field_value(session, &field.ty_ref, field_value)?,
        );
    }
    Ok(fields)
}
