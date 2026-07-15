use coflow_cft::{CftSchema, CftSchemaTypeRef};

pub(super) fn type_after_field_segment(
    schema: &CftSchema,
    actual_type: &str,
    field_name: &str,
) -> Option<CftSchemaTypeRef> {
    schema
        .field(actual_type, field_name)
        .map(|field| field.ty_ref.clone())
}

pub(super) fn type_after_field_segment_for_ref(
    schema: &CftSchema,
    current_type: &CftSchemaTypeRef,
    field_name: &str,
) -> Option<CftSchemaTypeRef> {
    match non_nullable(current_type) {
        CftSchemaTypeRef::Object(type_name) => {
            type_after_field_segment(schema, type_name, field_name)
        }
        _ => None,
    }
}

pub(super) fn concrete_type_for_block(
    schema: &CftSchema,
    expected_type: &CftSchemaTypeRef,
    type_marker: Option<&str>,
) -> CftSchemaTypeRef {
    let Some(type_marker) = type_marker else {
        return expected_type.clone();
    };
    let CftSchemaTypeRef::Object(expected_name) = non_nullable(expected_type) else {
        return expected_type.clone();
    };
    schema
        .resolve_type(type_marker)
        .filter(|_| schema.is_assignable(type_marker, expected_name))
        .map_or_else(
            || expected_type.clone(),
            |schema_type| CftSchemaTypeRef::Object(schema_type.name.clone()),
        )
}

pub(super) fn object_type_name<'a>(
    expected: Option<&'a CftSchemaTypeRef>,
    actual_type: &'a str,
) -> Option<&'a str> {
    match expected.map(non_nullable) {
        Some(CftSchemaTypeRef::Object(type_name)) => Some(type_name.as_str()),
        Some(CftSchemaTypeRef::RecordRef(_)) => None,
        Some(_) | None => Some(actual_type),
    }
}

pub(super) fn type_after_index_segment(
    current_type: &CftSchemaTypeRef,
) -> Option<CftSchemaTypeRef> {
    match non_nullable(current_type) {
        CftSchemaTypeRef::Array(inner) => Some((**inner).clone()),
        _ => None,
    }
}

pub(super) fn type_after_dict_key_segment(
    current_type: &CftSchemaTypeRef,
) -> Option<(CftSchemaTypeRef, CftSchemaTypeRef)> {
    match non_nullable(current_type) {
        CftSchemaTypeRef::Dict(key, item) => Some(((**key).clone(), (**item).clone())),
        _ => None,
    }
}

pub(super) fn dict_key_path_matches(
    key_type: &CftSchemaTypeRef,
    source_key: &str,
    path_key: &str,
) -> bool {
    if source_key == path_key {
        return true;
    }
    match non_nullable(key_type) {
        CftSchemaTypeRef::String if path_key.starts_with('"') => {
            serde_json::from_str::<String>(path_key).is_ok_and(|decoded| decoded == source_key)
        }
        CftSchemaTypeRef::Enum(enum_name) => path_key
            .strip_prefix(enum_name.as_str())
            .and_then(|rest| rest.strip_prefix('.'))
            .is_some_and(|variant| variant == source_key),
        CftSchemaTypeRef::Nullable(inner) => dict_key_path_matches(inner, source_key, path_key),
        _ => false,
    }
}

pub(super) fn non_nullable(ty: &CftSchemaTypeRef) -> &CftSchemaTypeRef {
    match ty {
        CftSchemaTypeRef::Nullable(inner) => non_nullable(inner),
        other => other,
    }
}
