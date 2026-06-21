//! Convert internal data-model values into wire types for the editor.

use coflow_data_model::{CfdDataModel, CfdDictKey, CfdRecord, CfdValue};

use crate::types::{DictEntry, DictKey, FieldCell, FieldValue};

pub fn record_to_field_cells(record: &CfdRecord, model: &CfdDataModel) -> Vec<FieldCell> {
    record
        .fields
        .iter()
        .map(|(name, value)| FieldCell {
            name: name.clone(),
            value: cfd_value_to_wire(value, model),
        })
        .collect()
}

pub fn cfd_value_to_wire(value: &CfdValue, model: &CfdDataModel) -> FieldValue {
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
        CfdValue::Object(boxed) => FieldValue::Object {
            actual_type: boxed.actual_type.clone(),
            fields: record_to_field_cells(boxed, model),
        },
        CfdValue::Ref { key, target } => {
            let target_record = model.record(*target);
            FieldValue::Ref {
                target_type: target_record
                    .map(|r| r.actual_type.clone())
                    .unwrap_or_default(),
                target_key: key.clone(),
                target_file: None,
            }
        }
        CfdValue::Array(items) => FieldValue::Array {
            items: items.iter().map(|i| cfd_value_to_wire(i, model)).collect(),
        },
        CfdValue::Dict(entries) => FieldValue::Dict {
            entries: entries
                .iter()
                .map(|(k, v)| DictEntry {
                    key: dict_key_to_wire(k),
                    value: cfd_value_to_wire(v, model),
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
