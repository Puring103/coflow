use coflow_api::{CftContainer, CftSchemaTypeRef};

pub(super) fn type_after_field_segment(
    schema: &CftContainer,
    actual_type: &str,
    field_name: &str,
) -> Option<CftSchemaTypeRef> {
    schema
        .resolve_type(actual_type)?
        .all_fields
        .iter()
        .find(|field| field.name == field_name)
        .map(|field| field.ty_ref.clone())
}

pub(super) fn type_after_field_segment_for_ref(
    schema: &CftContainer,
    current_type: &CftSchemaTypeRef,
    field_name: &str,
) -> Option<CftSchemaTypeRef> {
    match non_nullable(current_type) {
        CftSchemaTypeRef::Named(type_name) if schema.has_type(type_name) => {
            type_after_field_segment(schema, type_name, field_name)
        }
        _ => None,
    }
}

pub(super) fn object_type_name<'a>(
    expected: Option<&'a CftSchemaTypeRef>,
    actual_type: &'a str,
) -> Option<&'a str> {
    match expected.map(non_nullable) {
        Some(CftSchemaTypeRef::Named(type_name)) => Some(type_name.as_str()),
        Some(CftSchemaTypeRef::Ref(_)) => None,
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
    schema: &CftContainer,
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
        CftSchemaTypeRef::Named(enum_name) if schema.has_enum(enum_name) => path_key
            .strip_prefix(enum_name)
            .and_then(|rest| rest.strip_prefix('.'))
            .is_some_and(|variant| variant == source_key),
        CftSchemaTypeRef::Nullable(inner) => {
            dict_key_path_matches(schema, inner, source_key, path_key)
        }
        _ => false,
    }
}

pub(super) fn non_nullable(ty: &CftSchemaTypeRef) -> &CftSchemaTypeRef {
    match ty {
        CftSchemaTypeRef::Nullable(inner) => non_nullable(inner),
        other => other,
    }
}
