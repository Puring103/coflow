use crate::{CfdDictKey, CfdEnumValue, CfdValue};

use super::strings::string_needs_quotes;

/// Renders a runtime value into the same CFD text grammar accepted by
/// [`super::parse_cell`].
/// 所有值分支都有文本表示，渲染过程不产生可恢复错误。
pub fn render_cell_value(value: &CfdValue) -> String {
    match value {
        CfdValue::OptionNone => "None".to_string(),
        CfdValue::OptionSome(value) => render_cell_value(value),
        CfdValue::Bool(value) => value.to_string(),
        CfdValue::Int(value) => value.to_string(),
        CfdValue::Float(value) => (*value as f32).to_string(),
        CfdValue::String(value) => render_string(value),
        CfdValue::FormattedString(value) => value.source.clone(),
        CfdValue::Function(value) => value.source.clone(),
        CfdValue::Enum(value) => render_enum_value(value),
        CfdValue::Ref(target_key) => format!("&{target_key}"),
        CfdValue::Array(items) => render_array(items),
        CfdValue::Dict(entries) => render_dict(entries),
        CfdValue::Object(record) => render_object(record),
    }
}

fn render_array(items: &[CfdValue]) -> String {
    let mut out = String::from("[");
    for (idx, item) in items.iter().enumerate() {
        if idx > 0 {
            out.push_str(" | ");
        }
        out.push_str(&render_cell_value(item));
    }
    out.push(']');
    out
}

fn render_dict(entries: &[(CfdDictKey, CfdValue)]) -> String {
    let mut out = String::from("{");
    for (idx, (key, value)) in entries.iter().enumerate() {
        if idx > 0 {
            out.push_str(", ");
        }
        out.push_str(&render_dict_key(key));
        out.push_str(": ");
        out.push_str(&render_cell_value(value));
    }
    out.push('}');
    out
}

fn render_dict_key(key: &CfdDictKey) -> String {
    match key {
        CfdDictKey::String(value) => render_string(value),
        CfdDictKey::Int(value) => value.to_string(),
        CfdDictKey::Bool(value) => value.to_string(),
        CfdDictKey::Enum(value) => render_enum_value(value),
    }
}

fn render_object(record: &crate::CfdObject) -> String {
    let mut out = String::new();
    if !record.actual_type().is_empty() {
        out.push_str(record.actual_type());
    }
    out.push('{');
    for (idx, (field, value)) in record.fields().iter().enumerate() {
        if idx > 0 {
            out.push_str(", ");
        }
        out.push_str(field.as_str());
        out.push_str(": ");
        out.push_str(&render_cell_value(value));
    }
    out.push('}');
    out
}

fn render_enum_value(value: &CfdEnumValue) -> String {
    value
        .variant
        .as_ref()
        .map_or_else(|| value.value.to_string(), ToString::to_string)
}

pub(super) fn render_string(value: &str) -> String {
    if string_needs_quotes(value) || value.contains('"') || value.contains('\\') {
        quote_string(value)
    } else {
        value.to_string()
    }
}

fn quote_string(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}
