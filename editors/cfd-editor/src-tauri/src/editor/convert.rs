//! Convert internal data-model values into wire types for the editor.
//!
//! Top-level conversion needs the session's `key_to_file` map so cells that
//! reference (or are inherited from) other records can carry a file path
//! the front-end uses for jump-to-source. Nested object/array/dict values
//! flow through pure `cfd_value_to_wire` calls that fold this context with
//! the model.
use coflow_data_model::{CfdDataModel, CfdDictKey, CfdRecord, CfdRecordId, CfdValue};
use std::collections::HashMap;

use crate::editor::types::{DictEntry, DictKey, FieldCell, FieldValue, SpreadInfo};

/// Lookup table the converter consults when populating spread / ref source
/// metadata. Holds the session's `record_key → file_path` map so we don't
/// have to thread the entire `EditorSession` through.
pub struct WireContext<'a> {
    pub model: &'a CfdDataModel,
    pub key_to_file: &'a HashMap<String, String>,
}

/// Convert a top-level record into wire `FieldCell`s. Each cell whose value
/// originated from a `...spread` expansion carries a [`SpreadInfo`] payload
/// so the front-end can render it as inherited and dispatch edits to the
/// right place.
pub fn record_to_field_cells_for_session(
    record: &CfdRecord,
    model: &CfdDataModel,
    key_to_file: &HashMap<String, String>,
) -> Vec<FieldCell> {
    let ctx = WireContext { model, key_to_file };
    record
        .fields
        .iter()
        .map(|(name, value)| {
            let spread_info = spread_info_for_field(record, name, &ctx, &[]);
            FieldCell {
                name: name.clone(),
                value: cfd_value_to_wire(value, &ctx),
                is_spread: spread_info.is_some(),
                spread_info,
            }
        })
        .collect()
}

/// Convert a nested `CfdRecord` (inside `CfdValue::Object`) into cells.
/// `parent_path` is the field path inside the host record that leads to
/// this object; it's used to compose `spread_info.source_field_path` so the
/// front-end can show "inherited from `basic_monster.stats.attack`" rather
/// than just "inherited from `basic_monster`".
fn record_to_field_cells_nested(
    record: &CfdRecord,
    ctx: &WireContext<'_>,
    parent_path: &[String],
) -> Vec<FieldCell> {
    record
        .fields
        .iter()
        .map(|(name, value)| {
            let spread_info = spread_info_for_field(record, name, ctx, parent_path);
            FieldCell {
                name: name.clone(),
                value: cfd_value_to_wire_with_path(value, ctx, parent_path, name),
                is_spread: spread_info.is_some(),
                spread_info,
            }
        })
        .collect()
}

fn spread_info_for_field(
    record: &CfdRecord,
    field_name: &str,
    ctx: &WireContext<'_>,
    parent_path: &[String],
) -> Option<SpreadInfo> {
    let source_id = record.spread_field_sources.get(field_name).copied()?;
    spread_info_for_source(ctx, source_id, parent_path, field_name)
}

fn spread_info_for_source(
    ctx: &WireContext<'_>,
    source_id: CfdRecordId,
    parent_path: &[String],
    field_name: &str,
) -> Option<SpreadInfo> {
    let source = ctx.model.record(source_id)?;
    // `source_field_path` mirrors where in the source record the inherited
    // value lives. Top-level spreads (`...@T.k`) lift the source record's
    // field with the same name as our own; nested spreads inside an object
    // carry the same field name at the corresponding nesting depth.
    let mut source_field_path = parent_path.to_vec();
    source_field_path.push(field_name.to_string());
    Some(SpreadInfo {
        source_record_key: source.key.clone(),
        source_record_type: source.actual_type.clone(),
        source_record_file: ctx.key_to_file.get(&source.key).cloned(),
        source_field_path,
    })
}

pub fn cfd_value_to_wire(value: &CfdValue, ctx: &WireContext<'_>) -> FieldValue {
    cfd_value_to_wire_with_path(value, ctx, &[], "")
}

fn cfd_value_to_wire_with_path(
    value: &CfdValue,
    ctx: &WireContext<'_>,
    parent_path: &[String],
    field_name: &str,
) -> FieldValue {
    match value {
        CfdValue::Null => FieldValue::Null,
        CfdValue::Bool(v) => FieldValue::Bool { v: *v },
        CfdValue::Int(v) => FieldValue::Int { v: *v },
        CfdValue::Float(v) => FieldValue::Float { v: *v },
        CfdValue::String(v) => FieldValue::Str { v: v.clone() },
        CfdValue::Enum(e) => FieldValue::Enum {
            enum_name: e.enum_name.clone(),
            variant: e.variant.clone().unwrap_or_else(|| e.value.to_string()),
            int_value: e.value,
        },
        CfdValue::Object(boxed) => {
            let mut next = parent_path.to_vec();
            if !field_name.is_empty() {
                next.push(field_name.to_string());
            }
            FieldValue::Object {
                actual_type: boxed.actual_type.clone(),
                fields: record_to_field_cells_nested(boxed, ctx, &next),
            }
        }
        CfdValue::Ref {
            target_type,
            target_key,
        } => FieldValue::Ref {
            target_type: target_type.clone(),
            target_key: target_key.clone(),
            target_file: ctx.key_to_file.get(target_key).cloned(),
        },
        CfdValue::Array(items) => FieldValue::Array {
            items: items.iter().map(|i| cfd_value_to_wire(i, ctx)).collect(),
        },
        CfdValue::Dict(entries) => FieldValue::Dict {
            entries: entries
                .iter()
                .map(|(k, v)| DictEntry {
                    key: dict_key_to_wire(k),
                    value: cfd_value_to_wire(v, ctx),
                })
                .collect(),
        },
    }
}

fn dict_key_to_wire(key: &CfdDictKey) -> DictKey {
    match key {
        CfdDictKey::String(v) => DictKey::Str { v: v.clone() },
        CfdDictKey::Int(v) => DictKey::Int { v: *v },
        CfdDictKey::Enum(e) => DictKey::Enum {
            enum_name: e.enum_name.clone(),
            variant: e.variant.clone().unwrap_or_else(|| e.value.to_string()),
            int_value: e.value,
        },
    }
}
