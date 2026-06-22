//! Bridges between the editor's wire types (`FieldValue`, `FieldPathSegment`,
//! `DictKey`) and the runtime data-model values consumed by writers.
use coflow_api::{CfdDictKey as ApiCfdDictKey, CfdValue, RecordOrigin, WriteFieldPathSegment};
use coflow_cft::{CftContainer, CftSchemaDefaultValue, CftSchemaTypeRef};
use coflow_data_model::{
    CfdDataModel, CfdEnumValue, CfdRecord, CfdRecordId, CfdValue as DmCfdValue,
};
use std::collections::BTreeMap;

use crate::types::{DictKey, FieldPathSegment, FieldValue};

/// Map an editor wire `FieldPathSegment` to the api-level
/// `WriteFieldPathSegment` consumed by writers.
pub(super) fn field_path_segment_to_api(segment: &FieldPathSegment) -> WriteFieldPathSegment {
    match segment {
        FieldPathSegment::Field { name } => WriteFieldPathSegment::Field(name.clone()),
        FieldPathSegment::Index { i } => WriteFieldPathSegment::Index(*i),
    }
}

/// Convert a wire `FieldValue` into a runtime `CfdValue`.
///
/// `model` is consulted to resolve [`FieldValue::Ref`]s back to concrete
/// [`CfdRecordId`]s — without that, writers (most importantly `CfdWriter`)
/// cannot decide whether to emit `@Type.key` or `&key` and may produce
/// source text that fails to re-parse on next reload.
///
/// # Errors
/// Returns an error when a referenced target type/key pair does not match
/// any record in the model. Refusing the write is correct: silently rounding
/// a missing target to a sentinel id would let the writer corrupt the
/// underlying source on disk.
pub(super) fn field_value_to_cfd(
    value: &FieldValue,
    model: &CfdDataModel,
) -> Result<CfdValue, String> {
    match value {
        FieldValue::Null => Ok(DmCfdValue::Null),
        FieldValue::Bool { v } => Ok(DmCfdValue::Bool(*v)),
        FieldValue::Int { v } => Ok(DmCfdValue::Int(*v)),
        FieldValue::Float { v } => Ok(DmCfdValue::Float(*v)),
        FieldValue::Str { v } => Ok(DmCfdValue::String(v.clone())),
        FieldValue::Enum {
            enum_name,
            variant,
            int_value,
        } => Ok(DmCfdValue::Enum(CfdEnumValue {
            enum_name: enum_name.clone(),
            variant: Some(variant.clone()),
            value: *int_value,
        })),
        FieldValue::Object {
            actual_type,
            fields,
        } => {
            let mut converted = BTreeMap::new();
            for cell in fields {
                converted.insert(cell.name.clone(), field_value_to_cfd(&cell.value, model)?);
            }
            Ok(DmCfdValue::Object(Box::new(CfdRecord {
                key: String::new(),
                actual_type: actual_type.clone(),
                fields: converted,
                origin: RecordOrigin::None,
                spread_field_sources: BTreeMap::new(),
            })))
        }
        FieldValue::Ref {
            target_type,
            target_key,
            ..
        } => {
            let target = resolve_ref_target(model, target_type, target_key).ok_or_else(|| {
                format!(
                    "ref target `{target_type}.{target_key}` does not match any record in the model"
                )
            })?;
            Ok(DmCfdValue::Ref {
                key: target_key.clone(),
                target,
            })
        }
        FieldValue::Array { items } => {
            let mut converted = Vec::with_capacity(items.len());
            for item in items {
                converted.push(field_value_to_cfd(item, model)?);
            }
            Ok(DmCfdValue::Array(converted))
        }
        FieldValue::Dict { entries } => {
            let mut converted = Vec::with_capacity(entries.len());
            for e in entries {
                let k = match &e.key {
                    DictKey::Str { v } => ApiCfdDictKey::String(v.clone()),
                    DictKey::Int { v } => ApiCfdDictKey::Int(*v),
                    DictKey::Enum {
                        enum_name,
                        variant,
                        int_value,
                    } => ApiCfdDictKey::Enum(CfdEnumValue {
                        enum_name: enum_name.clone(),
                        variant: Some(variant.clone()),
                        value: *int_value,
                    }),
                };
                converted.push((k, field_value_to_cfd(&e.value, model)?));
            }
            Ok(DmCfdValue::Dict(converted))
        }
    }
}

/// Find the `CfdRecordId` for a (`target_type`, `target_key`) pair. Falls back
/// to a linear scan over `model.records()` if neither the concrete table nor
/// any polymorphic index has the key — covers the case where the wire
/// `target_type` is missing or wrong but the key is still uniquely
/// identifiable.
fn resolve_ref_target(
    model: &CfdDataModel,
    target_type: &str,
    target_key: &str,
) -> Option<CfdRecordId> {
    if !target_type.is_empty() {
        if let Some(id) = model.lookup(target_type, target_key) {
            return Some(id);
        }
    }
    model
        .records()
        .find(|(_, record)| record.key == target_key)
        .map(|(id, _)| id)
}

/// Best-effort default value for a schema-declared field. Used by
/// `make_default_object` when the front-end switches a `Ref` cell to inline
/// `Object` and needs a populated stub.
pub(super) fn default_value_for_ty(
    ty: &CftSchemaTypeRef,
    declared_default: Option<&CftSchemaDefaultValue>,
    schema: &CftContainer,
) -> FieldValue {
    if let Some(d) = declared_default {
        return default_from_schema_default(d, schema);
    }
    default_zero_for_ty(ty, schema)
}

fn default_from_schema_default(d: &CftSchemaDefaultValue, schema: &CftContainer) -> FieldValue {
    let _ = schema;
    match d {
        CftSchemaDefaultValue::Null => FieldValue::Null,
        CftSchemaDefaultValue::Int(v) => FieldValue::Int { v: *v },
        CftSchemaDefaultValue::Float(v) => FieldValue::Float { v: *v },
        CftSchemaDefaultValue::Bool(v) => FieldValue::Bool { v: *v },
        CftSchemaDefaultValue::String(v) => FieldValue::Str { v: v.clone() },
        CftSchemaDefaultValue::Enum {
            enum_name,
            variant,
            value,
        } => FieldValue::Enum {
            enum_name: enum_name.clone(),
            variant: variant.clone(),
            int_value: *value,
        },
        CftSchemaDefaultValue::EmptyArray => FieldValue::Array { items: Vec::new() },
        CftSchemaDefaultValue::EmptyObject => FieldValue::Dict {
            entries: Vec::new(),
        },
    }
}

fn default_zero_for_ty(ty: &CftSchemaTypeRef, schema: &CftContainer) -> FieldValue {
    match ty {
        CftSchemaTypeRef::Int => FieldValue::Int { v: 0 },
        CftSchemaTypeRef::Float => FieldValue::Float { v: 0.0 },
        CftSchemaTypeRef::Bool => FieldValue::Bool { v: false },
        CftSchemaTypeRef::String => FieldValue::Str { v: String::new() },
        CftSchemaTypeRef::Array(_) => FieldValue::Array { items: Vec::new() },
        CftSchemaTypeRef::Dict(_, _) => FieldValue::Dict {
            entries: Vec::new(),
        },
        CftSchemaTypeRef::Nullable(_) => FieldValue::Null,
        CftSchemaTypeRef::Named(name) => {
            if let Some(en) = schema.resolve_enum(name) {
                if let Some(first) = en.variants.first() {
                    return FieldValue::Enum {
                        enum_name: name.clone(),
                        variant: first.name.clone(),
                        int_value: first.value,
                    };
                }
            }
            FieldValue::Object {
                actual_type: name.clone(),
                fields: Vec::new(),
            }
        }
    }
}
