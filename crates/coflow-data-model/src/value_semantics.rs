use coflow_cft::{CftSchema, CftValueType};

use crate::diagnostic::CfdPath;
use crate::model::{CfdDictKey, CfdDomainId, CfdEnumValue, CfdRecordId, CfdValue};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueValidationMode {
    SourceFragment,
    Complete,
    Mutation,
}

#[derive(Debug, Clone, Copy)]
pub struct ValueValidationRequest<'a> {
    pub expected: &'a CftValueType,
    pub value: &'a CfdValue,
    pub mode: ValueValidationMode,
    pub pending_insert: Option<PendingInsertRef<'a>>,
}

impl<'a> ValueValidationRequest<'a> {
    #[must_use]
    pub const fn new(
        expected: &'a CftValueType,
        value: &'a CfdValue,
        mode: ValueValidationMode,
    ) -> Self {
        Self {
            expected,
            value,
            mode,
            pending_insert: None,
        }
    }

    #[must_use]
    pub const fn with_pending_insert(mut self, pending_insert: PendingInsertRef<'a>) -> Self {
        self.pending_insert = Some(pending_insert);
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CfdValueSemanticErrorKind {
    UnknownType,
    AbstractType,
    SingletonType,
    ObjectTypeMismatch,
    UnknownField,
    MissingRequiredField,
    TypeMismatch,
    InvalidEnumVariant,
    RefTargetNotFound,
    RefTargetTypeMismatch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CfdValueSemanticError {
    kind: CfdValueSemanticErrorKind,
    path: CfdPath,
    message: String,
}

impl CfdValueSemanticError {
    fn new(kind: CfdValueSemanticErrorKind, path: CfdPath, message: impl Into<String>) -> Self {
        Self {
            kind,
            path,
            message: message.into(),
        }
    }

    #[must_use]
    pub const fn kind(&self) -> CfdValueSemanticErrorKind {
        self.kind
    }

    #[must_use]
    pub const fn path(&self) -> &CfdPath {
        &self.path
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

/// Validates one complete or fragment CFD value against the canonical schema.
///
/// This is the semantic validation entry point shared by DataModel build and
/// runtime mutation preflight. Source decoding, spread application, default
/// materialization, and mutation expected-state checks stay outside it.
///
/// # Errors
///
/// Returns the first semantic error with a path relative to `request.value`.
pub fn validate_value_for_schema<C: CfdValueSemanticContext>(
    schema: &CftSchema,
    context: &C,
    request: ValueValidationRequest<'_>,
) -> Result<(), CfdValueSemanticError> {
    validate_value_inner(
        schema,
        context,
        request.expected,
        request.value,
        request.pending_insert,
        request.mode,
        CfdPath::root(),
    )
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
    validate_object_type_assignable_at(schema, expected_type, actual_type, CfdPath::root())
}

pub(crate) fn validate_dict_key_for_schema(
    schema: &CftSchema,
    expected: &CftValueType,
    value: &CfdDictKey,
) -> Result<(), CfdValueSemanticError> {
    validate_dict_key(schema, expected, value, CfdPath::root())
}

fn validate_object_type_assignable_at(
    schema: &CftSchema,
    expected_type: &str,
    actual_type: &str,
    path: CfdPath,
) -> Result<(), CfdValueSemanticError> {
    let Some(schema_type) = schema.resolve_type(actual_type) else {
        return Err(CfdValueSemanticError::new(
            CfdValueSemanticErrorKind::UnknownType,
            path,
            format!("unknown object type `{actual_type}`"),
        ));
    };
    if schema_type.is_abstract {
        return Err(CfdValueSemanticError::new(
            CfdValueSemanticErrorKind::AbstractType,
            path,
            format!("abstract object type `{actual_type}` cannot be instantiated"),
        ));
    }
    if schema_type.is_singleton {
        return Err(CfdValueSemanticError::new(
            CfdValueSemanticErrorKind::SingletonType,
            path,
            format!("singleton object type `{actual_type}` cannot be used as a field value"),
        ));
    }
    if !schema.is_assignable(actual_type, expected_type) {
        return Err(CfdValueSemanticError::new(
            CfdValueSemanticErrorKind::ObjectTypeMismatch,
            path,
            format!("type `{actual_type}` is not assignable to `{expected_type}`"),
        ));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn validate_value_inner<C: CfdValueSemanticContext>(
    schema: &CftSchema,
    context: &C,
    expected: &CftValueType,
    value: &CfdValue,
    pending_insert: Option<PendingInsertRef<'_>>,
    mode: ValueValidationMode,
    path: CfdPath,
) -> Result<(), CfdValueSemanticError> {
    match expected {
        CftValueType::Nullable(_) if matches!(value, CfdValue::Null) => Ok(()),
        CftValueType::Nullable(inner) => {
            validate_value_inner(schema, context, inner, value, pending_insert, mode, path)
        }
        CftValueType::Int => match value {
            CfdValue::Int(_) => Ok(()),
            _ => Err(type_mismatch("int", value, path)),
        },
        CftValueType::Float => match value {
            CfdValue::Float(float) if float.is_finite() => Ok(()),
            CfdValue::Float(_) => Err(CfdValueSemanticError::new(
                CfdValueSemanticErrorKind::TypeMismatch,
                path,
                "float value must be finite",
            )),
            _ => Err(type_mismatch("float", value, path)),
        },
        CftValueType::Bool => match value {
            CfdValue::Bool(_) => Ok(()),
            _ => Err(type_mismatch("bool", value, path)),
        },
        CftValueType::String => match value {
            CfdValue::String(_) => Ok(()),
            _ => Err(type_mismatch("string", value, path)),
        },
        CftValueType::Array(inner) => {
            validate_array(schema, context, inner, value, pending_insert, mode, path)
        }
        CftValueType::Dict(key, item) => validate_dict(
            schema,
            context,
            key,
            item,
            value,
            pending_insert,
            mode,
            path,
        ),
        CftValueType::RecordRef(expected_type) => {
            validate_ref_value(schema, context, expected_type, value, pending_insert, path)
        }
        CftValueType::Object(name) => {
            validate_object_value(schema, context, name, value, pending_insert, mode, path)
        }
        CftValueType::Enum(name) => match value {
            CfdValue::Enum(enum_value) => validate_enum(schema, name, enum_value, path),
            _ => Err(type_mismatch(&format!("enum `{name}`"), value, path)),
        },
    }
}

#[allow(clippy::too_many_arguments)]
fn validate_array<C: CfdValueSemanticContext>(
    schema: &CftSchema,
    context: &C,
    inner: &CftValueType,
    value: &CfdValue,
    pending_insert: Option<PendingInsertRef<'_>>,
    mode: ValueValidationMode,
    path: CfdPath,
) -> Result<(), CfdValueSemanticError> {
    let CfdValue::Array(items) = value else {
        return Err(type_mismatch("array", value, path));
    };
    for (index, item) in items.iter().enumerate() {
        validate_value_inner(
            schema,
            context,
            inner,
            item,
            pending_insert,
            mode,
            path.clone().index(index),
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn validate_dict<C: CfdValueSemanticContext>(
    schema: &CftSchema,
    context: &C,
    key: &CftValueType,
    item: &CftValueType,
    value: &CfdValue,
    pending_insert: Option<PendingInsertRef<'_>>,
    mode: ValueValidationMode,
    path: CfdPath,
) -> Result<(), CfdValueSemanticError> {
    let CfdValue::Dict(entries) = value else {
        return Err(type_mismatch("dict", value, path));
    };
    for (dict_key, item_value) in entries {
        let entry_path = path.clone().dict_key_value(dict_key);
        validate_dict_key(schema, key, dict_key, entry_path.clone())?;
        validate_value_inner(
            schema,
            context,
            item,
            item_value,
            pending_insert,
            mode,
            entry_path,
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
    path: CfdPath,
) -> Result<(), CfdValueSemanticError> {
    match value {
        CfdValue::Ref(target_key) => validate_ref_target(
            schema,
            context,
            expected_type,
            target_key,
            pending_insert,
            path,
        ),
        CfdValue::Object(_) => Err(CfdValueSemanticError::new(
            CfdValueSemanticErrorKind::TypeMismatch,
            path,
            "reference fields only allow record refs",
        )),
        _ => Err(type_mismatch(
            &format!("record ref for `&{expected_type}`"),
            value,
            path,
        )),
    }
}

#[allow(clippy::too_many_arguments)]
fn validate_object_value<C: CfdValueSemanticContext>(
    schema: &CftSchema,
    context: &C,
    expected_type: &str,
    value: &CfdValue,
    pending_insert: Option<PendingInsertRef<'_>>,
    mode: ValueValidationMode,
    path: CfdPath,
) -> Result<(), CfdValueSemanticError> {
    match value {
        CfdValue::Object(record) => {
            validate_object_type_assignable_at(
                schema,
                expected_type,
                record.actual_type(),
                path.clone(),
            )?;
            for (name, value) in record.fields() {
                let field_path = path.clone().field(name.as_str());
                let Some(field) = schema.field(record.actual_type(), name) else {
                    return Err(CfdValueSemanticError::new(
                        CfdValueSemanticErrorKind::UnknownField,
                        field_path,
                        format!("unknown field `{name}` on type `{}`", record.actual_type()),
                    ));
                };
                validate_value_inner(
                    schema,
                    context,
                    &field.value_type,
                    value,
                    pending_insert,
                    mode,
                    field_path,
                )?;
            }
            if mode != ValueValidationMode::SourceFragment {
                let schema_type = schema.resolve_type(record.actual_type()).ok_or_else(|| {
                    CfdValueSemanticError::new(
                        CfdValueSemanticErrorKind::UnknownType,
                        path.clone(),
                        format!("unknown object type `{}`", record.actual_type()),
                    )
                })?;
                for field in schema_type.all_fields() {
                    if field.default.is_none() && !record.fields().contains_key(field.name.as_str())
                    {
                        return Err(CfdValueSemanticError::new(
                            CfdValueSemanticErrorKind::MissingRequiredField,
                            path.clone().field(field.name.as_str()),
                            format!(
                                "missing required field `{}` on object type `{}`",
                                field.name,
                                record.actual_type()
                            ),
                        ));
                    }
                }
            }
            Ok(())
        }
        CfdValue::Ref(_) => Err(CfdValueSemanticError::new(
            CfdValueSemanticErrorKind::TypeMismatch,
            path,
            "inline object fields do not accept record refs",
        )),
        _ => Err(type_mismatch(
            &format!("object `{expected_type}`"),
            value,
            path,
        )),
    }
}

fn validate_dict_key(
    schema: &CftSchema,
    expected: &CftValueType,
    value: &CfdDictKey,
    path: CfdPath,
) -> Result<(), CfdValueSemanticError> {
    match (non_nullable(expected), value) {
        (CftValueType::String, CfdDictKey::String(_)) | (CftValueType::Int, CfdDictKey::Int(_)) => {
            Ok(())
        }
        (CftValueType::Enum(enum_name), CfdDictKey::Enum(enum_value)) => {
            validate_enum(schema, enum_name, enum_value, path)
        }
        _ => Err(CfdValueSemanticError::new(
            CfdValueSemanticErrorKind::TypeMismatch,
            path,
            "dict key does not match schema type",
        )),
    }
}

fn validate_enum(
    schema: &CftSchema,
    expected_enum: &str,
    value: &CfdEnumValue,
    path: CfdPath,
) -> Result<(), CfdValueSemanticError> {
    if value.enum_name.as_str() != expected_enum {
        return Err(CfdValueSemanticError::new(
            CfdValueSemanticErrorKind::TypeMismatch,
            path,
            format!(
                "expected enum `{expected_enum}`, got enum `{}`",
                value.enum_name
            ),
        ));
    }
    let Some(variant) = value.variant.as_deref() else {
        return Err(CfdValueSemanticError::new(
            CfdValueSemanticErrorKind::InvalidEnumVariant,
            path,
            format!(
                "enum `{expected_enum}` value {} has no declared variant",
                value.value
            ),
        ));
    };
    let Some(expected_value) = schema.enum_variant_value(expected_enum, variant) else {
        return Err(CfdValueSemanticError::new(
            CfdValueSemanticErrorKind::InvalidEnumVariant,
            path,
            format!("unknown enum variant `{expected_enum}.{variant}`"),
        ));
    };
    if value.value != expected_value {
        return Err(CfdValueSemanticError::new(
            CfdValueSemanticErrorKind::InvalidEnumVariant,
            path,
            format!(
                "enum value `{expected_enum}.{variant}` has value {}, expected {expected_value}",
                value.value
            ),
        ));
    }
    Ok(())
}

fn validate_ref_target<C: CfdValueSemanticContext>(
    schema: &CftSchema,
    context: &C,
    expected_type: &str,
    target_key: &str,
    pending_insert: Option<PendingInsertRef<'_>>,
    path: CfdPath,
) -> Result<(), CfdValueSemanticError> {
    let Some(domain) = context.type_domain_id(expected_type) else {
        return Err(CfdValueSemanticError::new(
            CfdValueSemanticErrorKind::UnknownType,
            path,
            format!("unknown reference target type `{expected_type}`"),
        ));
    };
    if let Some(target_id) = context.record_by_domain_key(domain, target_key) {
        let Some(actual_type) = context.record_actual_type(target_id) else {
            return Err(ref_not_found(expected_type, target_key, path));
        };
        if !schema.is_assignable(actual_type, expected_type) {
            return Err(CfdValueSemanticError::new(
                CfdValueSemanticErrorKind::RefTargetTypeMismatch,
                path,
                format!(
                    "ref target actual type `{actual_type}` is not assignable to `{expected_type}`"
                ),
            ));
        }
        return Ok(());
    }
    if let Some(actual_type) = context.pending_record_actual_type(domain, target_key) {
        if !schema.is_assignable(actual_type, expected_type) {
            return Err(CfdValueSemanticError::new(
                CfdValueSemanticErrorKind::RefTargetTypeMismatch,
                path,
                format!(
                    "ref target actual type `{actual_type}` is not assignable to `{expected_type}`"
                ),
            ));
        }
        return Ok(());
    }
    if let Some(pending) = pending_insert {
        if pending.key == target_key && schema.is_assignable(pending.actual_type, expected_type) {
            return Ok(());
        }
    }
    Err(ref_not_found(expected_type, target_key, path))
}

fn non_nullable(ty: &CftValueType) -> &CftValueType {
    match ty {
        CftValueType::Nullable(inner) => non_nullable(inner),
        other => other,
    }
}

fn type_mismatch(expected: &str, value: &CfdValue, path: CfdPath) -> CfdValueSemanticError {
    CfdValueSemanticError::new(
        CfdValueSemanticErrorKind::TypeMismatch,
        path,
        format!("expected {expected}, got {}", value_kind(value)),
    )
}

fn ref_not_found(expected_type: &str, target_key: &str, path: CfdPath) -> CfdValueSemanticError {
    CfdValueSemanticError::new(
        CfdValueSemanticErrorKind::RefTargetNotFound,
        path,
        format!("ref target `{expected_type}` with key `{target_key}` was not found"),
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
