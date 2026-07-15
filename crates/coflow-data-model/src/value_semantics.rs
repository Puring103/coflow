use coflow_cft::{CftSchemaTypeRef, CftSchema};

use crate::model::{CfdDictKey, CfdDomainId, CfdEnumValue, CfdRecordId, CfdValue};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CfdValueSemanticError {
    message: String,
}

impl CfdValueSemanticError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingInsertRef<'a> {
    pub actual_type: &'a str,
    pub key: &'a str,
}

pub trait CfdValueSemanticContext {
    fn type_domain_id(&self, type_name: &str) -> Option<CfdDomainId>;
    fn record_by_domain_key(&self, domain_id: CfdDomainId, key: &str) -> Option<CfdRecordId>;
    fn record_actual_type(&self, id: CfdRecordId) -> Option<&str>;

    fn pending_record_actual_type(&self, _domain_id: CfdDomainId, _key: &str) -> Option<&str> {
        None
    }
}

/// Validates that a complete CFD value matches a schema type and semantic context.
///
/// # Errors
///
/// Returns an error when the value shape does not match the schema type, when
/// an inline object omits a required field, when enum/object/ref semantics are
/// invalid, or when a referenced record cannot be resolved from the semantic
/// context.
pub fn validate_complete_value_for_schema<C: CfdValueSemanticContext>(
    schema: &CftSchema,
    context: &C,
    expected: &CftSchemaTypeRef,
    value: &CfdValue,
    pending_insert: Option<PendingInsertRef<'_>>,
) -> Result<(), CfdValueSemanticError> {
    validate_value_inner(
        schema,
        context,
        expected,
        value,
        pending_insert,
        ValueCompleteness::Complete,
    )
}

/// Validates a path-local CFD fragment without requiring omitted object fields.
///
/// # Errors
///
/// Returns an error when a provided value has the wrong shape or invalid
/// enum/object/ref semantics. Missing object fields are intentionally legal.
pub fn validate_fragment_value_for_schema<C: CfdValueSemanticContext>(
    schema: &CftSchema,
    context: &C,
    expected: &CftSchemaTypeRef,
    value: &CfdValue,
    pending_insert: Option<PendingInsertRef<'_>>,
) -> Result<(), CfdValueSemanticError> {
    validate_value_inner(
        schema,
        context,
        expected,
        value,
        pending_insert,
        ValueCompleteness::Fragment,
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ValueCompleteness {
    Complete,
    Fragment,
}

/// Validates that an object type can be instantiated where another type is expected.
///
/// # Errors
///
/// Returns an error when the actual type is unknown, abstract, singleton-only, or
/// not assignable to the expected type.
pub fn validate_object_type_assignable(
    schema: &CftSchema,
    expected_type: &str,
    actual_type: &str,
) -> Result<(), CfdValueSemanticError> {
    validate_object_type_assignable_in_view(schema, expected_type, actual_type)
}

fn validate_object_type_assignable_in_view(
    schema: &CftSchema,
    expected_type: &str,
    actual_type: &str,
) -> Result<(), CfdValueSemanticError> {
    let Some(schema_type) = schema.resolve_type(actual_type) else {
        return Err(CfdValueSemanticError::new(format!(
            "unknown object type `{actual_type}`"
        )));
    };
    if schema_type.is_abstract {
        return Err(CfdValueSemanticError::new(format!(
            "abstract object type `{actual_type}` cannot be instantiated"
        )));
    }
    if schema_type.is_singleton {
        return Err(CfdValueSemanticError::new(format!(
            "singleton object type `{actual_type}` cannot be used as a field value"
        )));
    }
    if !schema.is_assignable(actual_type, expected_type) {
        return Err(CfdValueSemanticError::new(format!(
            "type `{actual_type}` is not assignable to `{expected_type}`"
        )));
    }
    Ok(())
}

fn validate_value_inner<C: CfdValueSemanticContext>(
    schema: &CftSchema,
    context: &C,
    expected: &CftSchemaTypeRef,
    value: &CfdValue,
    pending_insert: Option<PendingInsertRef<'_>>,
    completeness: ValueCompleteness,
) -> Result<(), CfdValueSemanticError> {
    match expected {
        CftSchemaTypeRef::Nullable(_) if matches!(value, CfdValue::Null) => Ok(()),
        CftSchemaTypeRef::Nullable(inner) => {
            validate_value_inner(schema, context, inner, value, pending_insert, completeness)
        }
        CftSchemaTypeRef::Int => match value {
            CfdValue::Int(_) => Ok(()),
            _ => Err(type_mismatch("int", value)),
        },
        CftSchemaTypeRef::Float => match value {
            CfdValue::Float(float) if float.is_finite() => Ok(()),
            CfdValue::Float(_) => Err(CfdValueSemanticError::new("float value must be finite")),
            _ => Err(type_mismatch("float", value)),
        },
        CftSchemaTypeRef::Bool => match value {
            CfdValue::Bool(_) => Ok(()),
            _ => Err(type_mismatch("bool", value)),
        },
        CftSchemaTypeRef::String => match value {
            CfdValue::String(_) => Ok(()),
            _ => Err(type_mismatch("string", value)),
        },
        CftSchemaTypeRef::Array(inner) => {
            validate_array(schema, context, inner, value, pending_insert, completeness)
        }
        CftSchemaTypeRef::Dict(key, item) => validate_dict(
            schema,
            context,
            key,
            item,
            value,
            pending_insert,
            completeness,
        ),
        CftSchemaTypeRef::RecordRef(expected_type) => {
            validate_ref_value(schema, context, expected_type, value, pending_insert)
        }
        CftSchemaTypeRef::Object(name) => validate_object_value(
            schema,
            context,
            name,
            value,
            pending_insert,
            completeness,
        ),
        CftSchemaTypeRef::Enum(name) => match value {
            CfdValue::Enum(enum_value) => validate_enum(schema, name, enum_value),
            _ => Err(type_mismatch(&format!("enum `{name}`"), value)),
        },
    }
}

fn validate_array<C: CfdValueSemanticContext>(
    schema: &CftSchema,
    context: &C,
    inner: &CftSchemaTypeRef,
    value: &CfdValue,
    pending_insert: Option<PendingInsertRef<'_>>,
    completeness: ValueCompleteness,
) -> Result<(), CfdValueSemanticError> {
    let CfdValue::Array(items) = value else {
        return Err(type_mismatch("array", value));
    };
    for item in items {
        validate_value_inner(schema, context, inner, item, pending_insert, completeness)?;
    }
    Ok(())
}

fn validate_dict<C: CfdValueSemanticContext>(
    schema: &CftSchema,
    context: &C,
    key: &CftSchemaTypeRef,
    item: &CftSchemaTypeRef,
    value: &CfdValue,
    pending_insert: Option<PendingInsertRef<'_>>,
    completeness: ValueCompleteness,
) -> Result<(), CfdValueSemanticError> {
    let CfdValue::Dict(entries) = value else {
        return Err(type_mismatch("dict", value));
    };
    for (dict_key, item_value) in entries {
        validate_dict_key(schema, key, dict_key)?;
        validate_value_inner(
            schema,
            context,
            item,
            item_value,
            pending_insert,
            completeness,
        )?;
    }
    Ok(())
}

fn validate_ref_value<C: CfdValueSemanticContext>(
    schema: &CftSchema,
    context: &C,
    expected_type: &str,
    value: &CfdValue,
    pending_insert: Option<PendingInsertRef<'_>>,
) -> Result<(), CfdValueSemanticError> {
    match value {
        CfdValue::Ref(target_key) => {
            validate_ref_target(schema, context, expected_type, target_key, pending_insert)
        }
        CfdValue::Object(_) => Err(CfdValueSemanticError::new(
            "reference fields only allow record refs",
        )),
        _ => Err(type_mismatch(
            &format!("record ref for `&{expected_type}`"),
            value,
        )),
    }
}

fn validate_object_value<C: CfdValueSemanticContext>(
    schema: &CftSchema,
    context: &C,
    expected_type: &str,
    value: &CfdValue,
    pending_insert: Option<PendingInsertRef<'_>>,
    completeness: ValueCompleteness,
) -> Result<(), CfdValueSemanticError> {
    match value {
        CfdValue::Object(record) => {
            validate_object_type_assignable_in_view(schema, expected_type, record.actual_type())?;
            for (name, value) in record.fields() {
                let Some(field) = schema.field(record.actual_type(), name) else {
                    return Err(CfdValueSemanticError::new(format!(
                        "unknown field `{name}` on type `{}`",
                        record.actual_type()
                    )));
                };
                validate_value_inner(
                    schema,
                    context,
                    &field.ty_ref,
                    value,
                    pending_insert,
                    completeness,
                )?;
            }
            if completeness == ValueCompleteness::Complete {
                let schema_type = schema.resolve_type(record.actual_type()).ok_or_else(|| {
                    CfdValueSemanticError::new(format!(
                        "unknown object type `{}`",
                        record.actual_type()
                    ))
                })?;
                for field in schema_type.all_fields() {
                    if field.default.is_none()
                        && !record.fields().contains_key(field.name.as_str())
                    {
                        return Err(CfdValueSemanticError::new(format!(
                            "missing required field `{}` on object type `{}`",
                            field.name,
                            record.actual_type()
                        )));
                    }
                }
            }
            Ok(())
        }
        CfdValue::Ref(_) => Err(CfdValueSemanticError::new(
            "inline object fields do not accept record refs",
        )),
        _ => Err(type_mismatch(&format!("object `{expected_type}`"), value)),
    }
}

fn validate_dict_key(
    schema: &CftSchema,
    expected: &CftSchemaTypeRef,
    value: &CfdDictKey,
) -> Result<(), CfdValueSemanticError> {
    match (non_nullable(expected), value) {
        (CftSchemaTypeRef::String, CfdDictKey::String(_))
        | (CftSchemaTypeRef::Int, CfdDictKey::Int(_)) => Ok(()),
        (CftSchemaTypeRef::Enum(enum_name), CfdDictKey::Enum(enum_value)) => {
            validate_enum(schema, enum_name, enum_value)
        }
        _ => Err(CfdValueSemanticError::new(
            "dict key does not match schema type",
        )),
    }
}

fn validate_enum(
    schema: &CftSchema,
    expected_enum: &str,
    value: &CfdEnumValue,
) -> Result<(), CfdValueSemanticError> {
    if value.enum_name != expected_enum {
        return Err(CfdValueSemanticError::new(format!(
            "expected enum `{expected_enum}`, got enum `{}`",
            value.enum_name
        )));
    }
    let Some(variant) = value.variant.as_deref() else {
        return Err(CfdValueSemanticError::new(format!(
            "enum `{expected_enum}` value {} has no declared variant",
            value.value
        )));
    };
    let Some(expected_value) = schema.enum_variant_value(expected_enum, variant) else {
        return Err(CfdValueSemanticError::new(format!(
            "unknown enum variant `{expected_enum}.{variant}`"
        )));
    };
    if value.value != expected_value {
        return Err(CfdValueSemanticError::new(format!(
            "enum value `{expected_enum}.{variant}` has value {}, expected {expected_value}",
            value.value
        )));
    }
    Ok(())
}

fn validate_ref_target<C: CfdValueSemanticContext>(
    schema: &CftSchema,
    context: &C,
    expected_type: &str,
    target_key: &str,
    pending_insert: Option<PendingInsertRef<'_>>,
) -> Result<(), CfdValueSemanticError> {
    if target_key.is_empty() {
        return Err(CfdValueSemanticError::new(
            "reference key must not be empty",
        ));
    }
    let Some(domain) = context.type_domain_id(expected_type) else {
        return Err(CfdValueSemanticError::new(format!(
            "unknown reference target type `{expected_type}`"
        )));
    };
    if let Some(target_id) = context.record_by_domain_key(domain, target_key) {
        let Some(actual_type) = context.record_actual_type(target_id) else {
            return Err(ref_not_found(expected_type, target_key));
        };
        if !schema.is_assignable(actual_type, expected_type) {
            return Err(CfdValueSemanticError::new(format!(
                "ref target actual type `{actual_type}` is not assignable to `{expected_type}`"
            )));
        }
        return Ok(());
    }
    if let Some(actual_type) = context.pending_record_actual_type(domain, target_key) {
        if !schema.is_assignable(actual_type, expected_type) {
            return Err(CfdValueSemanticError::new(format!(
                "ref target actual type `{actual_type}` is not assignable to `{expected_type}`"
            )));
        }
        return Ok(());
    }
    if let Some(pending) = pending_insert {
        if pending.key == target_key && schema.is_assignable(pending.actual_type, expected_type) {
            return Ok(());
        }
    }
    Err(ref_not_found(expected_type, target_key))
}

fn non_nullable(ty: &CftSchemaTypeRef) -> &CftSchemaTypeRef {
    match ty {
        CftSchemaTypeRef::Nullable(inner) => non_nullable(inner),
        other => other,
    }
}

fn type_mismatch(expected: &str, value: &CfdValue) -> CfdValueSemanticError {
    CfdValueSemanticError::new(format!("expected {expected}, got {}", value_kind(value)))
}

fn ref_not_found(expected_type: &str, target_key: &str) -> CfdValueSemanticError {
    CfdValueSemanticError::new(format!(
        "ref target `{expected_type}` with key `{target_key}` was not found"
    ))
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
