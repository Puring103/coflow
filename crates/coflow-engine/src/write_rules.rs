use coflow_api::{Diagnostic, DiagnosticSet, Severity, WriteFieldPathSegment};
use coflow_cft::{CftContainer, CftSchemaTypeRef};
use coflow_data_model::{CfdPath, CfdPathSegment, CfdRecordId, CfdValue};

use crate::ProjectSession;

pub fn validate_record_key(key: &str, code: &'static str) -> Result<(), DiagnosticSet> {
    validate_record_key_for_stage(key, code, "WRITE")
}

fn validate_record_key_for_stage(
    key: &str,
    code: &'static str,
    stage: &'static str,
) -> Result<(), DiagnosticSet> {
    if let Some(reason) = coflow_cft::record_key_ident_error(key) {
        return Err(one_error(
            code,
            stage,
            format!("record key `{key}` is invalid: {reason}"),
        ));
    }
    Ok(())
}

pub fn ensure_record_key_available(
    session: &ProjectSession,
    actual_type: &str,
    key: &str,
    current_record: Option<CfdRecordId>,
    code: &'static str,
    stage: &'static str,
) -> Result<(), DiagnosticSet> {
    validate_record_key_for_stage(key, code, stage)?;
    let Some(domain) = session.model.type_domain_id(actual_type) else {
        return Err(one_error(
            code,
            stage,
            format!("unknown type `{actual_type}`"),
        ));
    };
    let Some(existing_id) = session.model.record_by_domain_key(domain, key) else {
        return Ok(());
    };
    if current_record == Some(existing_id) {
        return Ok(());
    }
    let existing = session.model.record(existing_id).ok_or_else(|| {
        one_error(
            code,
            stage,
            format!("key `{key}` already exists in `{actual_type}` inheritance domain"),
        )
    })?;
    Err(one_error(
        code,
        stage,
        format!(
            "key `{key}` already exists in `{actual_type}` inheritance domain as `{}.{}`",
            existing.actual_type(),
            existing.key
        ),
    ))
}

pub fn expected_type_for_write_path(
    schema: &CftContainer,
    actual_type: &str,
    path: &[WriteFieldPathSegment],
    code: &'static str,
    stage: &'static str,
) -> Result<CftSchemaTypeRef, DiagnosticSet> {
    let cfd_path = write_path_to_cfd_path(path, code, stage)?;
    expected_type_for_cfd_path(schema, actual_type, &cfd_path.segments, code, stage)
}

pub fn expected_type_for_cfd_path(
    schema: &CftContainer,
    actual_type: &str,
    path: &[CfdPathSegment],
    code: &'static str,
    stage: &'static str,
) -> Result<CftSchemaTypeRef, DiagnosticSet> {
    if path.is_empty() {
        return Err(one_error(code, stage, "field path must not be empty"));
    }
    let mut current = CftSchemaTypeRef::Named(actual_type.to_string());
    for segment in path {
        current = match segment {
            CfdPathSegment::Field(field) => {
                let CftSchemaTypeRef::Named(type_name) = non_nullable(&current) else {
                    return Err(one_error(
                        code,
                        stage,
                        format!("field `{field}` cannot be selected from this value"),
                    ));
                };
                let schema_type = schema
                    .resolve_type(type_name)
                    .ok_or_else(|| one_error(code, stage, format!("unknown type `{type_name}`")))?;
                schema_type
                    .all_fields
                    .iter()
                    .find(|schema_field| schema_field.name == *field)
                    .map(|schema_field| schema_field.ty_ref.clone())
                    .ok_or_else(|| {
                        one_error(
                            code,
                            stage,
                            format!("unknown field `{field}` on type `{type_name}`"),
                        )
                    })?
            }
            CfdPathSegment::Index(index) => {
                let CftSchemaTypeRef::Array(inner) = non_nullable(&current) else {
                    return Err(one_error(
                        code,
                        stage,
                        format!("array index `{index}` cannot be selected from this value"),
                    ));
                };
                (**inner).clone()
            }
            CfdPathSegment::DictKey(key) => {
                let CftSchemaTypeRef::Dict(_, item) = non_nullable(&current) else {
                    return Err(one_error(
                        code,
                        stage,
                        format!("dict key `{key}` cannot be selected from this value"),
                    ));
                };
                (**item).clone()
            }
        };
    }
    Ok(current)
}

pub fn validate_value_for_write(
    session: &ProjectSession,
    expected: &CftSchemaTypeRef,
    value: &CfdValue,
    code: &'static str,
    stage: &'static str,
) -> Result<(), DiagnosticSet> {
    validate_value_for_write_inner(session, expected, value, None, code, stage)
}

pub fn validate_value_for_insert(
    session: &ProjectSession,
    inserted_actual_type: &str,
    inserted_key: &str,
    expected: &CftSchemaTypeRef,
    value: &CfdValue,
    code: &'static str,
    stage: &'static str,
) -> Result<(), DiagnosticSet> {
    validate_value_for_write_inner(
        session,
        expected,
        value,
        Some((inserted_actual_type, inserted_key)),
        code,
        stage,
    )
}

fn validate_value_for_write_inner(
    session: &ProjectSession,
    expected: &CftSchemaTypeRef,
    value: &CfdValue,
    pending_insert: Option<(&str, &str)>,
    code: &'static str,
    stage: &'static str,
) -> Result<(), DiagnosticSet> {
    match expected {
        CftSchemaTypeRef::Nullable(_) if matches!(value, CfdValue::Null) => Ok(()),
        CftSchemaTypeRef::Nullable(inner) => {
            validate_value_for_write_inner(session, inner, value, pending_insert, code, stage)
        }
        CftSchemaTypeRef::Int => match value {
            CfdValue::Int(_) => Ok(()),
            _ => Err(type_mismatch(code, stage, "int", value)),
        },
        CftSchemaTypeRef::Float => match value {
            CfdValue::Float(float) if float.is_finite() => Ok(()),
            CfdValue::Float(_) => Err(one_error(code, stage, "float value must be finite")),
            _ => Err(type_mismatch(code, stage, "float", value)),
        },
        CftSchemaTypeRef::Bool => match value {
            CfdValue::Bool(_) => Ok(()),
            _ => Err(type_mismatch(code, stage, "bool", value)),
        },
        CftSchemaTypeRef::String => match value {
            CfdValue::String(_) => Ok(()),
            _ => Err(type_mismatch(code, stage, "string", value)),
        },
        CftSchemaTypeRef::Array(inner) => {
            validate_array_for_write(session, inner, value, pending_insert, code, stage)
        }
        CftSchemaTypeRef::Dict(key, item) => {
            validate_dict_for_write(session, key, item, value, pending_insert, code, stage)
        }
        CftSchemaTypeRef::Ref(expected_type) => {
            validate_ref_value_for_write(session, expected_type, value, pending_insert, code, stage)
        }
        CftSchemaTypeRef::Named(name) => {
            validate_named_value_for_write(session, name, value, pending_insert, code, stage)
        }
    }
}

fn validate_array_for_write(
    session: &ProjectSession,
    inner: &CftSchemaTypeRef,
    value: &CfdValue,
    pending_insert: Option<(&str, &str)>,
    code: &'static str,
    stage: &'static str,
) -> Result<(), DiagnosticSet> {
    let CfdValue::Array(items) = value else {
        return Err(type_mismatch(code, stage, "array", value));
    };
    for item in items {
        validate_value_for_write_inner(session, inner, item, pending_insert, code, stage)?;
    }
    Ok(())
}

fn validate_dict_for_write(
    session: &ProjectSession,
    key: &CftSchemaTypeRef,
    item: &CftSchemaTypeRef,
    value: &CfdValue,
    pending_insert: Option<(&str, &str)>,
    code: &'static str,
    stage: &'static str,
) -> Result<(), DiagnosticSet> {
    let CfdValue::Dict(entries) = value else {
        return Err(type_mismatch(code, stage, "dict", value));
    };
    for (dict_key, item_value) in entries {
        validate_dict_key_for_write(&session.schema, key, dict_key, code, stage)?;
        validate_value_for_write_inner(session, item, item_value, pending_insert, code, stage)?;
    }
    Ok(())
}

fn validate_ref_value_for_write(
    session: &ProjectSession,
    expected_type: &str,
    value: &CfdValue,
    pending_insert: Option<(&str, &str)>,
    code: &'static str,
    stage: &'static str,
) -> Result<(), DiagnosticSet> {
    match value {
        CfdValue::Ref(target_key) => validate_ref_target_for_write(
            session,
            expected_type,
            target_key,
            pending_insert,
            code,
            stage,
        ),
        CfdValue::Object(_) => Err(one_error(
            code,
            stage,
            "reference fields only allow record refs",
        )),
        _ => Err(type_mismatch(
            code,
            stage,
            &format!("record ref for `&{expected_type}`"),
            value,
        )),
    }
}

fn validate_named_value_for_write(
    session: &ProjectSession,
    name: &str,
    value: &CfdValue,
    pending_insert: Option<(&str, &str)>,
    code: &'static str,
    stage: &'static str,
) -> Result<(), DiagnosticSet> {
    if session.schema.has_enum(name) {
        return match value {
            CfdValue::Enum(enum_value) => {
                validate_enum_for_write(&session.schema, name, enum_value, code, stage)
            }
            _ => Err(type_mismatch(code, stage, &format!("enum `{name}`"), value)),
        };
    }
    validate_object_value_for_write(session, name, value, pending_insert, code, stage)
}

fn validate_object_value_for_write(
    session: &ProjectSession,
    expected_type: &str,
    value: &CfdValue,
    pending_insert: Option<(&str, &str)>,
    code: &'static str,
    stage: &'static str,
) -> Result<(), DiagnosticSet> {
    match value {
        CfdValue::Object(record) => {
            ensure_object_type_assignable(
                &session.schema,
                expected_type,
                record.actual_type(),
                code,
                stage,
            )?;
            for (name, value) in record.fields() {
                let Some(field) =
                    session
                        .schema
                        .resolve_type(record.actual_type())
                        .and_then(|schema_type| {
                            schema_type
                                .all_fields
                                .iter()
                                .find(|field| field.name == *name)
                        })
                else {
                    return Err(one_error(
                        code,
                        stage,
                        format!("unknown field `{name}` on type `{}`", record.actual_type()),
                    ));
                };
                validate_value_for_write_inner(
                    session,
                    &field.ty_ref,
                    value,
                    pending_insert,
                    code,
                    stage,
                )?;
            }
            Ok(())
        }
        CfdValue::Ref(_) => Err(one_error(
            code,
            stage,
            "inline object fields do not accept record refs",
        )),
        _ => Err(type_mismatch(
            code,
            stage,
            &format!("object `{expected_type}`"),
            value,
        )),
    }
}

fn validate_dict_key_for_write(
    schema: &CftContainer,
    expected: &CftSchemaTypeRef,
    value: &coflow_data_model::CfdDictKey,
    code: &'static str,
    stage: &'static str,
) -> Result<(), DiagnosticSet> {
    match (non_nullable(expected), value) {
        (CftSchemaTypeRef::String, coflow_data_model::CfdDictKey::String(_))
        | (CftSchemaTypeRef::Int, coflow_data_model::CfdDictKey::Int(_)) => Ok(()),
        (CftSchemaTypeRef::Named(enum_name), coflow_data_model::CfdDictKey::Enum(enum_value))
            if schema.has_enum(enum_name) =>
        {
            validate_enum_for_write(schema, enum_name, enum_value, code, stage)
        }
        _ => Err(one_error(
            code,
            stage,
            "dict key does not match schema type",
        )),
    }
}

fn validate_enum_for_write(
    schema: &CftContainer,
    expected_enum: &str,
    value: &coflow_data_model::CfdEnumValue,
    code: &'static str,
    stage: &'static str,
) -> Result<(), DiagnosticSet> {
    if value.enum_name != expected_enum {
        return Err(one_error(
            code,
            stage,
            format!(
                "expected enum `{expected_enum}`, got enum `{}`",
                value.enum_name
            ),
        ));
    }
    let Some(variant) = value.variant.as_deref() else {
        return Err(one_error(
            code,
            stage,
            format!(
                "enum `{expected_enum}` value {} has no declared variant",
                value.value
            ),
        ));
    };
    let Some(expected_value) = schema.enum_variant_value(expected_enum, variant) else {
        return Err(one_error(
            code,
            stage,
            format!("unknown enum variant `{expected_enum}.{variant}`"),
        ));
    };
    if value.value != expected_value {
        return Err(one_error(
            code,
            stage,
            format!(
                "enum value `{expected_enum}.{variant}` has value {}, expected {expected_value}",
                value.value
            ),
        ));
    }
    Ok(())
}

fn validate_ref_target_for_write(
    session: &ProjectSession,
    expected_type: &str,
    target_key: &str,
    pending_insert: Option<(&str, &str)>,
    code: &'static str,
    stage: &'static str,
) -> Result<(), DiagnosticSet> {
    if target_key.is_empty() {
        return Err(one_error(code, stage, "reference key must not be empty"));
    }
    let Some(domain) = session.model.type_domain_id(expected_type) else {
        return Err(one_error(
            code,
            stage,
            format!("unknown reference target type `{expected_type}`"),
        ));
    };
    if let Some(target_id) = session.model.record_by_domain_key(domain, target_key) {
        let Some(target) = session.model.record(target_id) else {
            return Err(one_error(
                code,
                stage,
                format!("ref target `{expected_type}` with key `{target_key}` was not found"),
            ));
        };
        if !session
            .schema
            .is_assignable(target.actual_type(), expected_type)
        {
            return Err(one_error(
                code,
                stage,
                format!(
                    "ref target actual type `{}` is not assignable to `{expected_type}`",
                    target.actual_type()
                ),
            ));
        }
        return Ok(());
    }
    if let Some((inserted_actual_type, inserted_key)) = pending_insert {
        if inserted_key == target_key
            && session
                .schema
                .is_assignable(inserted_actual_type, expected_type)
        {
            return Ok(());
        }
    }
    Err(one_error(
        code,
        stage,
        format!("ref target `{expected_type}` with key `{target_key}` was not found"),
    ))
}

pub fn ensure_object_type_assignable(
    schema: &CftContainer,
    expected_type: &str,
    actual_type: &str,
    code: &'static str,
    stage: &'static str,
) -> Result<(), DiagnosticSet> {
    let Some(schema_type) = schema.resolve_type(actual_type) else {
        return Err(one_error(
            code,
            stage,
            format!("unknown object type `{actual_type}`"),
        ));
    };
    if schema_type.is_abstract {
        return Err(one_error(
            code,
            stage,
            format!("abstract object type `{actual_type}` cannot be instantiated"),
        ));
    }
    if schema_type.is_singleton {
        return Err(one_error(
            code,
            stage,
            format!("singleton object type `{actual_type}` cannot be used as a field value"),
        ));
    }
    if !schema.is_assignable(actual_type, expected_type) {
        return Err(one_error(
            code,
            stage,
            format!("type `{actual_type}` is not assignable to `{expected_type}`"),
        ));
    }
    Ok(())
}

pub fn write_path_to_cfd_path(
    path: &[WriteFieldPathSegment],
    code: &'static str,
    stage: &'static str,
) -> Result<CfdPath, DiagnosticSet> {
    if path.is_empty() {
        return Err(one_error(code, stage, "field path must not be empty"));
    }
    Ok(path
        .iter()
        .fold(CfdPath::root(), |path, segment| match segment {
            WriteFieldPathSegment::Field(field) => path.field(field.clone()),
            WriteFieldPathSegment::Index(index) => path.index(*index),
            WriteFieldPathSegment::DictKey(key) => path.dict_key(key.clone()),
        }))
}

pub fn cfd_path_to_write_path(path: &[CfdPathSegment]) -> Vec<WriteFieldPathSegment> {
    path.iter()
        .map(|segment| match segment {
            CfdPathSegment::Field(field) => WriteFieldPathSegment::Field(field.clone()),
            CfdPathSegment::Index(index) => WriteFieldPathSegment::Index(*index),
            CfdPathSegment::DictKey(key) => WriteFieldPathSegment::DictKey(key.clone()),
        })
        .collect()
}

fn non_nullable(ty: &CftSchemaTypeRef) -> &CftSchemaTypeRef {
    match ty {
        CftSchemaTypeRef::Nullable(inner) => non_nullable(inner),
        other => other,
    }
}

fn type_mismatch(
    code: &'static str,
    stage: &'static str,
    expected: &str,
    value: &CfdValue,
) -> DiagnosticSet {
    one_error(
        code,
        stage,
        format!("expected {expected}, got {}", value_kind(value)),
    )
}

const fn value_kind(value: &CfdValue) -> &'static str {
    match value {
        CfdValue::Null => "null",
        CfdValue::Bool(_) => "bool",
        CfdValue::Int(_) => "int",
        CfdValue::Float(_) => "float",
        CfdValue::String(_) => "string",
        CfdValue::Enum(_) => "enum",
        CfdValue::Object(_) => "object",
        CfdValue::Ref(_) => "record ref",
        CfdValue::Array(_) => "array",
        CfdValue::Dict(_) => "dict",
    }
}

fn one_error(code: &'static str, stage: &'static str, message: impl Into<String>) -> DiagnosticSet {
    DiagnosticSet::one(Diagnostic {
        code: code.to_string(),
        stage: stage.to_string(),
        severity: Severity::Error,
        message: message.into(),
        primary: None,
        related: Vec::new(),
    })
}
