use std::collections::{BTreeMap, BTreeSet};

use coflow_api::{CftContainer, DiagnosticSet};
use coflow_cft::{CftFieldMeta, CftSchemaDefaultValue, CftSchemaTypeRef, CftSchemaView};
use coflow_data_model::{CfdEnumValue, CfdObject, CfdRecord, CfdValue, RecordOrigin};

use super::{non_nullable, one_mutation_error, DefaultMaterialization};

pub(super) fn default_record_for_type(
    schema: &CftContainer,
    type_name: &str,
    materialization: DefaultMaterialization,
) -> Result<CfdRecord, DiagnosticSet> {
    let schema = CftSchemaView::new(schema);
    ensure_type_can_materialize(&schema, type_name)?;
    let mut stack = BTreeSet::new();
    let fields =
        default_fields_for_type_inner(&schema, type_name, materialization, &mut stack, None)?;
    Ok(CfdRecord {
        key: String::new(),
        object: CfdObject::new(type_name, fields),
        origin: RecordOrigin::None,
    })
}

pub fn default_value_for_type_ref(
    schema: &CftContainer,
    ty: &CftSchemaTypeRef,
    materialization: DefaultMaterialization,
) -> Result<CfdValue, DiagnosticSet> {
    let schema = CftSchemaView::new(schema);
    let mut stack = BTreeSet::new();
    default_value_for_ty(&schema, ty, None, materialization, &mut stack)
}

pub(super) fn default_missing_fields_for_type(
    schema: &CftContainer,
    type_name: &str,
    materialization: DefaultMaterialization,
    provided_names: &BTreeSet<String>,
) -> Result<BTreeMap<String, CfdValue>, DiagnosticSet> {
    let schema = CftSchemaView::new(schema);
    let mut stack = BTreeSet::new();
    default_fields_for_type_inner(
        &schema,
        type_name,
        materialization,
        &mut stack,
        Some(provided_names),
    )
}

fn default_fields_for_type_inner(
    schema: &CftSchemaView,
    type_name: &str,
    materialization: DefaultMaterialization,
    stack: &mut BTreeSet<String>,
    skip_fields: Option<&BTreeSet<String>>,
) -> Result<BTreeMap<String, CfdValue>, DiagnosticSet> {
    let Some(schema_type) = schema.type_meta(type_name) else {
        return Err(one_mutation_error(
            "MUTATION-TYPE",
            format!("unknown type `{type_name}`"),
        ));
    };
    if schema_type.is_abstract {
        return Err(one_mutation_error(
            "MUTATION-DEFAULT",
            format!("abstract object type `{type_name}` cannot be default materialized"),
        ));
    }
    if schema_type.is_singleton {
        return Err(one_mutation_error(
            "MUTATION-DEFAULT",
            format!("singleton object type `{type_name}` cannot be default materialized"),
        ));
    }
    if !stack.insert(type_name.to_string()) {
        return if materialization == DefaultMaterialization::Minimal {
            Err(one_mutation_error(
                "MUTATION-DEFAULT",
                format!("required inline object type `{type_name}` is recursive"),
            ))
        } else {
            Ok(BTreeMap::new())
        };
    }
    let mut fields = BTreeMap::new();
    for field in schema.full_fields(&schema_type.name).unwrap_or(&[]) {
        if skip_fields.is_some_and(|skip_fields| skip_fields.contains(&field.name)) {
            continue;
        }
        let value = match materialization {
            DefaultMaterialization::Minimal => default_minimal_for_field(schema, field, stack)?,
            DefaultMaterialization::EditableShape => Some(default_value_for_ty(
                schema,
                &field.ty_ref,
                field.default.as_ref(),
                materialization,
                stack,
            )?),
        };
        if let Some(value) = value {
            fields.insert(field.name.clone(), value);
        }
    }
    stack.remove(type_name);
    Ok(fields)
}

fn default_minimal_for_field(
    schema: &CftSchemaView,
    field: &CftFieldMeta,
    stack: &mut BTreeSet<String>,
) -> Result<Option<CfdValue>, DiagnosticSet> {
    if field.default.is_some() {
        return Ok(None);
    }
    match non_nullable(&field.ty_ref) {
        CftSchemaTypeRef::Ref(name) => Err(one_mutation_error(
            "MUTATION-DEFAULT",
            format!(
                "field `{}` of type `&{name}` has no schema default; provide an explicit value",
                field.name
            ),
        )),
        CftSchemaTypeRef::Named(name) if schema.has_type(name) => {
            ensure_type_can_materialize(schema, name)?;
            let fields = default_fields_for_type_inner(
                schema,
                name,
                DefaultMaterialization::Minimal,
                stack,
                None,
            )?;
            Ok(Some(CfdValue::Object(Box::new(CfdObject::new(
                name.clone(),
                fields,
            )))))
        }
        CftSchemaTypeRef::Named(name) => Err(one_mutation_error(
            "MUTATION-DEFAULT",
            format!(
                "field `{}` of type `{name}` has no schema default; provide an explicit value",
                field.name
            ),
        )),
        _ => default_zero_for_ty_inner(schema, &field.ty_ref, stack).map(Some),
    }
}

fn default_value_for_ty(
    schema: &CftSchemaView,
    ty: &CftSchemaTypeRef,
    declared_default: Option<&CftSchemaDefaultValue>,
    materialization: DefaultMaterialization,
    stack: &mut BTreeSet<String>,
) -> Result<CfdValue, DiagnosticSet> {
    if let Some(default) = declared_default {
        return default_from_schema_default(schema, ty, default, materialization, stack);
    }
    default_zero_for_ty_inner(schema, ty, stack)
}

fn default_from_schema_default(
    schema: &CftSchemaView,
    ty: &CftSchemaTypeRef,
    default: &CftSchemaDefaultValue,
    materialization: DefaultMaterialization,
    stack: &mut BTreeSet<String>,
) -> Result<CfdValue, DiagnosticSet> {
    match default {
        CftSchemaDefaultValue::Null => Ok(CfdValue::Null),
        CftSchemaDefaultValue::Int(value) => Ok(CfdValue::Int(*value)),
        CftSchemaDefaultValue::Float(value) => Ok(CfdValue::Float(*value)),
        CftSchemaDefaultValue::Bool(value) => Ok(CfdValue::Bool(*value)),
        CftSchemaDefaultValue::String(value) => Ok(CfdValue::String(value.clone())),
        CftSchemaDefaultValue::Enum {
            enum_name,
            variant,
            value,
        } => Ok(CfdValue::Enum(
            schema.enum_value_from_int(enum_name, *value).map_or_else(
                || CfdEnumValue {
                    enum_name: enum_name.clone(),
                    variant: Some(variant.clone()),
                    value: *value,
                },
                |value| CfdEnumValue {
                    enum_name: value.enum_name,
                    variant: value.variant,
                    value: value.value,
                },
            ),
        )),
        CftSchemaDefaultValue::EmptyArray => Ok(CfdValue::Array(Vec::new())),
        CftSchemaDefaultValue::EmptyObject => match non_nullable(ty) {
            CftSchemaTypeRef::Named(name) if schema.has_type(name) => {
                let fields =
                    default_fields_for_type_inner(schema, name, materialization, stack, None)?;
                Ok(CfdValue::Object(Box::new(CfdObject::new(
                    name.clone(),
                    fields,
                ))))
            }
            CftSchemaTypeRef::Dict(_, _) => Ok(CfdValue::Dict(Vec::new())),
            _ => default_zero_for_ty_inner(schema, ty, stack),
        },
    }
}

fn default_zero_for_ty_inner(
    schema: &CftSchemaView,
    ty: &CftSchemaTypeRef,
    stack: &mut BTreeSet<String>,
) -> Result<CfdValue, DiagnosticSet> {
    match ty {
        CftSchemaTypeRef::Int => Ok(CfdValue::Int(0)),
        CftSchemaTypeRef::Float => Ok(CfdValue::Float(0.0)),
        CftSchemaTypeRef::Bool => Ok(CfdValue::Bool(false)),
        CftSchemaTypeRef::String => Ok(CfdValue::String(String::new())),
        CftSchemaTypeRef::Ref(_) | CftSchemaTypeRef::Nullable(_) => Ok(CfdValue::Null),
        CftSchemaTypeRef::Array(_) => Ok(CfdValue::Array(Vec::new())),
        CftSchemaTypeRef::Dict(_, _) => Ok(CfdValue::Dict(Vec::new())),
        CftSchemaTypeRef::Named(name) if schema.is_schema_enum(name) => {
            let value = schema
                .enum_meta(name)
                .and_then(|enm| enm.all_variants.first());
            Ok(value.map_or_else(
                || {
                    CfdValue::Enum(CfdEnumValue {
                        enum_name: name.clone(),
                        variant: None,
                        value: 0,
                    })
                },
                |variant| {
                    CfdValue::Enum(CfdEnumValue {
                        enum_name: name.clone(),
                        variant: Some(variant.name.clone()),
                        value: variant.value,
                    })
                },
            ))
        }
        CftSchemaTypeRef::Named(name) => {
            ensure_type_can_materialize(schema, name)?;
            let fields = default_fields_for_type_inner(
                schema,
                name,
                DefaultMaterialization::EditableShape,
                stack,
                None,
            )?;
            Ok(CfdValue::Object(Box::new(CfdObject::new(
                name.clone(),
                fields,
            ))))
        }
    }
}

fn ensure_type_can_materialize(
    schema: &CftSchemaView,
    type_name: &str,
) -> Result<(), DiagnosticSet> {
    let Some(schema_type) = schema.type_meta(type_name) else {
        return Err(one_mutation_error(
            "MUTATION-TYPE",
            format!("unknown type `{type_name}`"),
        ));
    };
    if schema_type.is_abstract {
        return Err(one_mutation_error(
            "MUTATION-DEFAULT",
            format!("abstract object type `{type_name}` cannot be default materialized"),
        ));
    }
    if schema_type.is_singleton {
        return Err(one_mutation_error(
            "MUTATION-DEFAULT",
            format!("singleton object type `{type_name}` cannot be default materialized"),
        ));
    }
    Ok(())
}
