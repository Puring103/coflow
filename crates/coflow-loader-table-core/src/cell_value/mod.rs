//! Schema-guided parser for Coflow cell value text.

#![cfg_attr(
    not(test),
    deny(
        clippy::dbg_macro,
        clippy::expect_used,
        clippy::panic,
        clippy::panic_in_result_fn,
        clippy::todo,
        clippy::unimplemented,
        clippy::unreachable,
        clippy::unwrap_used
    )
)]
#![allow(clippy::missing_const_for_fn, clippy::similar_names, clippy::use_self)]

mod diagnostics;
mod markers;
mod objects;
mod refs;
mod render;
mod scan;
mod strings;
mod types;

use coflow_cft::CftContainer;
use coflow_data_model::{CfdInputDictKey, CfdInputValue};
use diagnostics::{missing_boundary, syntax, type_mismatch};
pub use diagnostics::{CellValueDiagnostic, CellValueDiagnostics, CellValueErrorCode};
use objects::parse_object;
use refs::parse_ref;
pub use render::{render_cell_value, CellRenderError};
use scan::{find_top_level_char, split_top_level, strip_outer_pair};
use strings::parse_string;
use types::CellType;

#[derive(Debug, Clone, PartialEq)]
pub enum ParsedCell {
    Omitted,
    Value(CfdInputValue),
}

/// Parses one cell value using a CFT declared type as context.
///
/// # Errors
///
/// Returns diagnostics when the cell text does not match the declared type.
pub fn parse_cell(
    schema: &CftContainer,
    declared_type: &str,
    text: &str,
) -> Result<ParsedCell, CellValueDiagnostics> {
    let declared_type = CellType::parse(schema, declared_type)?;
    let text = text.trim();
    if text.is_empty() || text == "_" {
        return Ok(ParsedCell::Omitted);
    }
    parse_value(schema, &declared_type, text, ValueContext::Root).map(ParsedCell::Value)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ValueContext {
    Root,
    Nested,
}

impl ValueContext {
    fn is_root(self) -> bool {
        matches!(self, Self::Root)
    }
}

fn parse_value(
    schema: &CftContainer,
    ty: &CellType,
    text: &str,
    context: ValueContext,
) -> Result<CfdInputValue, CellValueDiagnostics> {
    let text = text.trim();
    if let CellType::Nullable(inner) = ty {
        return if text == "null" {
            Ok(CfdInputValue::Null)
        } else {
            parse_value(schema, inner, text, context)
        };
    }
    if text == "null" {
        return Err(type_mismatch(&ty.display()));
    }
    match ty {
        CellType::Int => Ok(CfdInputValue::Int(
            text.parse::<i64>().map_err(|_| type_mismatch("int"))?,
        )),
        CellType::Float => {
            let value = text.parse::<f64>().map_err(|_| type_mismatch("float"))?;
            if value.is_finite() {
                Ok(CfdInputValue::Float(value))
            } else {
                Err(type_mismatch("finite float"))
            }
        }
        CellType::Bool => match text.to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" | "y" => Ok(CfdInputValue::Bool(true)),
            "false" | "0" | "no" | "n" => Ok(CfdInputValue::Bool(false)),
            _ => Err(type_mismatch("bool")),
        },
        CellType::String => parse_string(text).map(CfdInputValue::String),
        CellType::Enum(enum_name) => parse_enum(schema, enum_name, text),
        CellType::Ref(type_name) => parse_ref(type_name, text),
        CellType::Type(type_name) => parse_object(schema, type_name, text, context),
        CellType::Array(inner) => parse_array(schema, inner, text, context),
        CellType::Dict(key, value) => parse_dict(schema, key, value, text, context),
        CellType::Nullable(inner) => parse_value(schema, inner, text, context),
    }
}

fn parse_enum(
    schema: &CftContainer,
    enum_name: &str,
    text: &str,
) -> Result<CfdInputValue, CellValueDiagnostics> {
    let variant = text
        .strip_prefix(enum_name)
        .and_then(|rest| rest.strip_prefix('.'))
        .map_or(text, |variant| variant);
    let Some(schema_enum) = schema.resolve_enum(enum_name) else {
        return Err(type_mismatch(enum_name));
    };
    if schema_enum
        .variants
        .iter()
        .any(|schema_variant| schema_variant.name == variant)
    {
        Ok(CfdInputValue::enum_variant(enum_name, variant))
    } else {
        Err(CellValueDiagnostics {
            diagnostics: vec![CellValueDiagnostic {
                code: CellValueErrorCode::InvalidEnumVariant,
                message: format!("unknown enum variant `{enum_name}.{variant}`"),
            }],
        })
    }
}

fn parse_array(
    schema: &CftContainer,
    inner: &CellType,
    text: &str,
    context: ValueContext,
) -> Result<CfdInputValue, CellValueDiagnostics> {
    let explicit = strip_outer_pair(text, '[', ']');
    if explicit.is_none() && !context.is_root() {
        return Err(missing_boundary("nested array must use `[]`"));
    }
    let content = explicit.map_or(text, |inner| inner).trim();
    if content.is_empty() {
        return Ok(CfdInputValue::Array(Vec::new()));
    }

    let mut out = Vec::new();
    for item in split_top_level(content, '|')? {
        reject_comma_array_item(inner, item)?;
        out.push(parse_value(schema, inner, item, ValueContext::Nested)?);
    }
    Ok(CfdInputValue::Array(out))
}

fn reject_comma_array_item(inner: &CellType, item: &str) -> Result<(), CellValueDiagnostics> {
    if find_top_level_char(item, ',')?.is_some() {
        return match inner {
            CellType::Type(_) | CellType::Dict(_, _) => Err(missing_boundary(
                "array elements with composite values must use boundaries",
            )),
            _ => Err(syntax("array elements must be separated with `|`")),
        };
    }
    Ok(())
}

fn parse_dict(
    schema: &CftContainer,
    key: &CellType,
    value: &CellType,
    text: &str,
    context: ValueContext,
) -> Result<CfdInputValue, CellValueDiagnostics> {
    let explicit = strip_outer_pair(text, '{', '}');
    if explicit.is_none() && !context.is_root() {
        return Err(missing_boundary("nested dict must use `{}`"));
    }
    let content = explicit.map_or(text, |inner| inner).trim();
    if content.is_empty() {
        return Ok(CfdInputValue::dict(std::iter::empty()));
    }

    let mut out = Vec::new();
    for entry in split_top_level(content, ',')? {
        let Some(colon) = find_top_level_char(entry, ':')? else {
            return Err(syntax("dict entry is missing `:`"));
        };
        let key_text = entry[..colon].trim();
        let value_text = entry[colon + 1..].trim();
        out.push((
            parse_dict_key(schema, key, key_text)?,
            parse_value(schema, value, value_text, ValueContext::Nested)?,
        ));
    }
    Ok(CfdInputValue::dict(out))
}

fn parse_dict_key(
    schema: &CftContainer,
    key: &CellType,
    text: &str,
) -> Result<CfdInputDictKey, CellValueDiagnostics> {
    let text = text.trim();
    match key {
        CellType::String => parse_string(text).map(CfdInputDictKey::String),
        CellType::Int => Ok(CfdInputDictKey::Int(
            text.parse::<i64>().map_err(|_| type_mismatch("int key"))?,
        )),
        CellType::Enum(enum_name) => {
            let CfdInputValue::EnumVariant { variant, .. } = parse_enum(schema, enum_name, text)?
            else {
                return Err(type_mismatch("enum key"));
            };
            Ok(CfdInputDictKey::enum_variant(enum_name, variant))
        }
        CellType::Nullable(inner) => parse_dict_key(schema, inner, text),
        other => Err(type_mismatch(&format!("dict key {}", other.display()))),
    }
}
