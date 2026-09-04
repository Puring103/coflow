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

use crate::LoadedValueDraft;
use coflow_language::cft::{CftSchema, CftValueType};
use collections::{parse_array, parse_dict};
use diagnostics::type_mismatch;
pub use diagnostics::{CellValueDiagnostic, CellValueDiagnostics, CellValueErrorCode};
use objects::parse_object;
use refs::parse_ref;
use scan::strip_outer_pair;
pub use render::{render_cell_value, CellRenderError};
pub(crate) use strings::parse_automatic_formatted_string;
use strings::parse_string;
use types::CellType;

#[derive(Debug, Clone, PartialEq)]
pub enum ParsedCell {
    Omitted,
    Value(LoadedValueDraft),
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

/// Parses one cell using an already compiled schema type.
///
/// # Errors
///
/// Returns diagnostics when the cell text does not match the declared type.
pub fn parse_schema_cell(
    schema: &CftSchema,
    declared_type: &CftValueType,
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
) -> Result<LoadedValueDraft, CellValueDiagnostics> {
    let text = text.trim();
    match ty {
        CellType::Option(_) if text == "None" => Ok(LoadedValueDraft::OptionNone),
        CellType::Option(inner) => {
            let value = constructor_inner(text, "Some").unwrap_or(text);
            parse_value(schema, inner, value, ValueContext::Nested)
                .map(|value| LoadedValueDraft::OptionSome(Box::new(value)))
        }
        CellType::Result(ok, _) if text.starts_with("Ok") => constructor_inner(text, "Ok")
            .ok_or_else(|| type_mismatch(&ty.display()))
            .and_then(|value| parse_value(schema, ok, value, ValueContext::Nested))
            .map(|value| LoadedValueDraft::ResultOk(Box::new(value))),
        CellType::Result(_, error) => constructor_inner(text, "Err")
            .ok_or_else(|| type_mismatch(&ty.display()))
            .and_then(|value| parse_value(schema, error, value, ValueContext::Nested))
            .map(|value| LoadedValueDraft::ResultErr(Box::new(value))),
        CellType::Int => Ok(LoadedValueDraft::Int(
            text.parse::<i64>().map_err(|_| type_mismatch("int"))?,
        )),
        CellType::Float => {
            let value = text.parse::<f64>().map_err(|_| type_mismatch("float"))?;
            if value.is_finite() {
                Ok(LoadedValueDraft::Float(value))
            } else {
                Err(type_mismatch("finite float"))
            }
        }
        CellType::Bool => match text {
            "true" => Ok(LoadedValueDraft::Bool(true)),
            "false" => Ok(LoadedValueDraft::Bool(false)),
            _ => Err(type_mismatch("bool")),
        },
        CellType::String if text.contains('{') => parse_automatic_formatted_string(text)?
            .map_or_else(
                || parse_string(text).map(LoadedValueDraft::String),
                |value| Ok(LoadedValueDraft::FormattedString(value)),
            ),
        CellType::String => parse_string(text).map(LoadedValueDraft::String),
        CellType::Enum(enum_name) => parse_enum(schema, enum_name, text),
        CellType::Ref(type_name) => parse_ref(type_name, text),
        CellType::Type(type_name) => parse_object(schema, type_name, text, context),
        CellType::Array(inner) => parse_array(schema, inner, text, context),
        CellType::Dict(key, value) => parse_dict(schema, key, value, text, context),
        CellType::Unsupported(display) => Err(type_mismatch(display)),
    }
}

fn constructor_inner<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    let rest = text.strip_prefix(name)?.trim_start();
    strip_outer_pair(rest, '(', ')')
}

pub(crate) fn parse_enum(
    schema: &CftSchema,
    enum_name: &str,
    text: &str,
) -> Result<LoadedValueDraft, CellValueDiagnostics> {
    let Some(schema_enum) = schema.resolve_enum(enum_name) else {
        return Err(type_mismatch(enum_name));
    };
    if schema_enum.is_flag {
        if let Ok(value) = text.parse::<i64>() {
            return Ok(LoadedValueDraft::enum_value(enum_name, value));
        }
    }
    parse_enum_variant(schema, enum_name, text)
}

pub(crate) fn parse_enum_variant(
    schema: &CftSchema,
    enum_name: &str,
    text: &str,
) -> Result<LoadedValueDraft, CellValueDiagnostics> {
    let variant = text
        .strip_prefix(enum_name)
        .and_then(|rest| rest.strip_prefix("::"))
        .map_or(text, |variant| variant);
    let Some(schema_enum) = schema.resolve_enum(enum_name) else {
        return Err(type_mismatch(enum_name));
    };
    if schema_enum
        .variants
        .iter()
        .any(|schema_variant| schema_variant.name.as_str() == variant)
    {
        Ok(LoadedValueDraft::enum_variant(enum_name, variant))
    } else {
        Err(CellValueDiagnostics {
            diagnostics: vec![CellValueDiagnostic {
                code: CellValueErrorCode::InvalidEnumVariant,
                message: format!("unknown enum variant `{enum_name}::{variant}`"),
            }],
        })
    }
}
