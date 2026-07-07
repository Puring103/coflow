use coflow_data_model::{CfdDictKey, CfdEnumValue, CfdValue};
use std::fmt;

use super::strings::string_needs_quotes;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellRenderError {
    AnonymousEnum,
    NestedObject,
}

impl fmt::Display for CellRenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AnonymousEnum => {
                write!(f, "anonymous enum values cannot be rendered as table cells")
            }
            Self::NestedObject => {
                write!(f, "nested object values cannot be rendered as table cells")
            }
        }
    }
}

impl std::error::Error for CellRenderError {}

/// Renders a runtime value into the same table-cell text grammar accepted by
/// [`super::parse_cell`].
///
/// # Errors
///
/// Returns an error when the runtime value cannot be represented without
/// schema context.
pub fn render_cell_value(value: &CfdValue) -> Result<String, CellRenderError> {
    match value {
        CfdValue::Null => Ok(String::new()),
        CfdValue::Bool(value) => Ok(value.to_string()),
        CfdValue::Int(value) => Ok(value.to_string()),
        CfdValue::Float(value) => Ok(value.to_string()),
        CfdValue::String(value) => Ok(render_string(value)),
        CfdValue::Enum(value) => render_enum_value(value),
        CfdValue::Ref(target_key) => Ok(format!("&{target_key}")),
        CfdValue::Array(items) => render_array(items),
        CfdValue::Dict(entries) => render_dict(entries),
        CfdValue::Object(record) => render_object(record),
    }
}

fn render_array(items: &[CfdValue]) -> Result<String, CellRenderError> {
    let mut out = String::from("[");
    for (idx, item) in items.iter().enumerate() {
        if idx > 0 {
            out.push_str(" | ");
        }
        out.push_str(&render_cell_value(item)?);
    }
    out.push(']');
    Ok(out)
}

fn render_dict(entries: &[(CfdDictKey, CfdValue)]) -> Result<String, CellRenderError> {
    let mut out = String::from("{");
    for (idx, (key, value)) in entries.iter().enumerate() {
        if idx > 0 {
            out.push_str(", ");
        }
        out.push_str(&render_dict_key(key)?);
        out.push_str(": ");
        out.push_str(&render_cell_value(value)?);
    }
    out.push('}');
    Ok(out)
}

fn render_dict_key(key: &CfdDictKey) -> Result<String, CellRenderError> {
    match key {
        CfdDictKey::String(value) => Ok(render_string(value)),
        CfdDictKey::Int(value) => Ok(value.to_string()),
        CfdDictKey::Enum(value) => render_enum_value(value),
    }
}

fn render_object(record: &coflow_data_model::CfdObject) -> Result<String, CellRenderError> {
    let mut out = String::new();
    if !record.actual_type().is_empty() {
        out.push_str(record.actual_type());
    }
    out.push('{');
    for (idx, (field, value)) in record.fields().iter().enumerate() {
        if idx > 0 {
            out.push_str(", ");
        }
        out.push_str(field);
        out.push_str(": ");
        out.push_str(&render_cell_value(value)?);
    }
    out.push('}');
    Ok(out)
}

fn render_enum_value(value: &CfdEnumValue) -> Result<String, CellRenderError> {
    value.variant.clone().ok_or(CellRenderError::AnonymousEnum)
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
