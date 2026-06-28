//! Schema-driven default value builders consumed by `make_default_object`.

use coflow_cft::{CftContainer, CftSchemaDefaultValue, CftSchemaTypeRef};
use coflow_data_model::{CfdEnumValue, CfdRecord, CfdValue};
use std::collections::BTreeMap;

use coflow_data_model::RecordOrigin;

/// Best-effort default value for a schema-declared field. Used by
/// `make_default_object` when the front-end switches a `Ref` cell to inline
/// `Object` and needs a populated stub.
pub(super) fn default_value_for_ty(
    ty: &CftSchemaTypeRef,
    declared_default: Option<&CftSchemaDefaultValue>,
    schema: &CftContainer,
) -> CfdValue {
    if let Some(d) = declared_default {
        return default_from_schema_default(ty, d, schema);
    }
    default_zero_for_ty(ty, schema)
}

fn default_from_schema_default(
    ty: &CftSchemaTypeRef,
    d: &CftSchemaDefaultValue,
    schema: &CftContainer,
) -> CfdValue {
    match d {
        CftSchemaDefaultValue::Null => CfdValue::Null,
        CftSchemaDefaultValue::Int(v) => CfdValue::Int(*v),
        CftSchemaDefaultValue::Float(v) => CfdValue::Float(*v),
        CftSchemaDefaultValue::Bool(v) => CfdValue::Bool(*v),
        CftSchemaDefaultValue::String(v) => CfdValue::String(v.clone()),
        CftSchemaDefaultValue::Enum {
            enum_name,
            variant,
            value,
        } => CfdValue::Enum(CfdEnumValue {
            enum_name: enum_name.clone(),
            variant: Some(variant.clone()),
            value: *value,
        }),
        CftSchemaDefaultValue::EmptyArray => CfdValue::Array(Vec::new()),
        CftSchemaDefaultValue::EmptyObject => match ty {
            CftSchemaTypeRef::Nullable(inner) => default_from_schema_default(inner, d, schema),
            CftSchemaTypeRef::Named(name) if schema.has_type(name) => {
                CfdValue::Object(Box::new(CfdRecord {
                    key: String::new(),
                    actual_type: name.clone(),
                    fields: BTreeMap::new(),
                    origin: RecordOrigin::None,
                    spread_field_sources: BTreeMap::new(),
                }))
            }
            _ => CfdValue::Dict(Vec::new()),
        },
    }
}

fn default_zero_for_ty(ty: &CftSchemaTypeRef, schema: &CftContainer) -> CfdValue {
    match ty {
        CftSchemaTypeRef::Int => CfdValue::Int(0),
        CftSchemaTypeRef::Float => CfdValue::Float(0.0),
        CftSchemaTypeRef::Bool => CfdValue::Bool(false),
        CftSchemaTypeRef::String => CfdValue::String(String::new()),
        CftSchemaTypeRef::Array(_) => CfdValue::Array(Vec::new()),
        CftSchemaTypeRef::Dict(_, _) => CfdValue::Dict(Vec::new()),
        CftSchemaTypeRef::Nullable(_) => CfdValue::Null,
        CftSchemaTypeRef::Named(name) => {
            if let Some(en) = schema.resolve_enum(name) {
                if let Some(first) = en.variants.first() {
                    return CfdValue::Enum(CfdEnumValue {
                        enum_name: name.clone(),
                        variant: Some(first.name.clone()),
                        value: first.value,
                    });
                }
            }
            CfdValue::Object(Box::new(CfdRecord {
                key: String::new(),
                actual_type: name.clone(),
                fields: BTreeMap::new(),
                origin: RecordOrigin::None,
                spread_field_sources: BTreeMap::new(),
            }))
        }
    }
}
