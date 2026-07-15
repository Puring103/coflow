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

mod collections;
mod diagnostics;
mod markers;
mod objects;
mod refs;
mod render;
mod scan;
mod strings;
mod types;

use coflow_cft::{CftSchema, CftSchemaTypeRef};
use coflow_data_model::CfdInputValue;
use collections::{parse_array, parse_dict};
use diagnostics::type_mismatch;
pub use diagnostics::{CellValueDiagnostic, CellValueDiagnostics, CellValueErrorCode};
use objects::parse_object;
use refs::parse_ref;
pub use render::{render_cell_value, CellRenderError};
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
    schema: &CftSchema,
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

pub(crate) fn parse_schema_cell(
    schema: &CftSchema,
    declared_type: &CftSchemaTypeRef,
    text: &str,
) -> Result<ParsedCell, CellValueDiagnostics> {
    let declared_type = CellType::from_schema_type(declared_type);
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
    schema: &CftSchema,
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

pub(super) fn parse_enum(
    schema: &CftSchema,
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
        .any(|schema_variant| schema_variant.name.as_str() == variant)
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
