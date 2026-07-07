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
mod render;
mod scan;
mod types;

use coflow_cft::{record_key_ident_error, CftContainer};
use coflow_data_model::{CfdInputDictKey, CfdInputValue};
use diagnostics::{missing_boundary, reference_needs_marker, syntax, type_mismatch};
pub use diagnostics::{CellValueDiagnostic, CellValueDiagnostics, CellValueErrorCode};
pub use render::{render_cell_value, CellRenderError};
use scan::{find_marker_open_brace, find_top_level_char, split_top_level, strip_outer_pair};
use std::collections::{BTreeMap, BTreeSet};
use types::{full_fields, CellType, FieldMeta};
use unicode_ident::{is_xid_continue, is_xid_start};

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

fn parse_object(
    schema: &CftContainer,
    expected_type: &str,
    text: &str,
    context: ValueContext,
) -> Result<CfdInputValue, CellValueDiagnostics> {
    let expected_fields = full_fields(schema, expected_type)?;
    if text.trim().starts_with('@') {
        return Err(syntax(
            "typed and path references are no longer supported; use `&key`",
        ));
    }
    if text.trim().starts_with('&') {
        return Err(type_mismatch(expected_type));
    }
    if looks_like_bare_record_key(text) {
        return Err(reference_needs_marker(text));
    }

    let ObjectContent {
        actual_type,
        content,
        has_boundary,
    } = object_content(text);
    if !context.is_root() && actual_type.is_none() && !has_boundary {
        return Err(missing_boundary("nested object must use `{}`"));
    }
    if let Some(actual) = &actual_type {
        validate_actual_type(schema, expected_type, actual)?;
    }
    let fields = if let Some(actual) = actual_type.as_deref() {
        full_fields(schema, actual)?
    } else {
        expected_fields
    };
    let content = content.trim();
    if content.is_empty() {
        return Ok(object_value(actual_type, std::iter::empty()));
    }

    let parts = split_top_level(content, ',')?;
    let colon_positions = parts
        .iter()
        .map(|part| find_top_level_char(part, ':'))
        .collect::<Result<Vec<_>, _>>()?;
    let known_field_names = fields
        .iter()
        .map(|field| field.name.as_str())
        .collect::<BTreeSet<_>>();
    let all_parts_have_colon = colon_positions.iter().all(Option::is_some);
    let has_known_named_part = parts.iter().zip(&colon_positions).any(|(part, colon)| {
        colon.is_some_and(|colon| known_field_names.contains(part[..colon].trim()))
    });
    let is_named = all_parts_have_colon;
    let is_mixed = !all_parts_have_colon && has_known_named_part;
    if is_mixed {
        return Err(CellValueDiagnostics {
            diagnostics: vec![CellValueDiagnostic {
                code: CellValueErrorCode::MixedObjectStyle,
                message: "object cannot mix named and positional fields".to_string(),
            }],
        });
    }

    if is_named {
        parse_named_object(schema, actual_type, &fields, &parts, &colon_positions)
    } else {
        parse_positional_object(schema, actual_type, &fields, &parts)
    }
}

fn parse_ref(expected_type: &str, text: &str) -> Result<CfdInputValue, CellValueDiagnostics> {
    let text = text.trim();
    let Some(key) = text.strip_prefix('&') else {
        if text.starts_with('@') {
            return Err(syntax(
                "typed and path references are no longer supported; use `&key`",
            ));
        }
        if looks_like_bare_record_key(text) {
            return Err(reference_needs_marker(text));
        }
        return Err(type_mismatch(&format!("&{expected_type}")));
    };
    if key.contains('.') || key.contains('[') || key.contains(']') {
        return Err(syntax("record references do not support paths"));
    }
    if key.trim() != key {
        return Err(syntax("direct reference key cannot contain whitespace"));
    }
    if key.is_empty() {
        return Err(syntax("reference key is missing"));
    }
    if let Some(reason) = record_key_ident_error(key) {
        return Err(syntax(format!("invalid reference key `{key}`: {reason}")));
    }
    Ok(CfdInputValue::record_ref(key))
}

fn looks_like_bare_record_key(text: &str) -> bool {
    let text = text.trim();
    !text.is_empty()
        && !matches!(text, "_" | "null")
        && is_type_marker_name(text)
        && !text.starts_with('{')
        && !text.starts_with('[')
        && !text.starts_with('"')
        && !text.contains(',')
        && !text.contains(':')
        && !text.contains('{')
        && !text.contains('}')
        && !text.contains('[')
        && !text.contains(']')
        && text.chars().next().is_some_and(|ch| ch != '@')
}

fn validate_actual_type(
    schema: &CftContainer,
    expected_type: &str,
    actual_type: &str,
) -> Result<(), CellValueDiagnostics> {
    let Some(actual_schema_type) = schema.resolve_type(actual_type) else {
        return Err(CellValueDiagnostics {
            diagnostics: vec![CellValueDiagnostic {
                code: CellValueErrorCode::UnknownType,
                message: format!("unknown type `{actual_type}`"),
            }],
        });
    };
    if actual_schema_type.is_abstract {
        return Err(CellValueDiagnostics {
            diagnostics: vec![CellValueDiagnostic {
                code: CellValueErrorCode::AbstractObjectType,
                message: format!("abstract type `{actual_type}` cannot be instantiated"),
            }],
        });
    }
    if !schema.is_assignable(actual_type, expected_type) {
        return Err(CellValueDiagnostics {
            diagnostics: vec![CellValueDiagnostic {
                code: CellValueErrorCode::ObjectTypeMismatch,
                message: format!("type `{actual_type}` is not assignable to `{expected_type}`"),
            }],
        });
    }
    Ok(())
}

fn parse_named_object(
    schema: &CftContainer,
    actual_type: Option<String>,
    fields: &[FieldMeta],
    parts: &[&str],
    colon_positions: &[Option<usize>],
) -> Result<CfdInputValue, CellValueDiagnostics> {
    let fields_by_name = fields
        .iter()
        .map(|field| (field.name.as_str(), field))
        .collect::<BTreeMap<_, _>>();
    let mut out = BTreeMap::new();
    let mut seen = BTreeSet::new();
    for (part, colon) in parts.iter().zip(colon_positions) {
        let Some(colon) = colon else {
            continue;
        };
        let name = part[..*colon].trim();
        let value_text = part[*colon + 1..].trim();
        let Some(field) = fields_by_name.get(name) else {
            return Err(CellValueDiagnostics {
                diagnostics: vec![CellValueDiagnostic {
                    code: CellValueErrorCode::UnknownField,
                    message: format!("unknown field `{name}`"),
                }],
            });
        };
        if !seen.insert(name.to_string()) {
            return Err(CellValueDiagnostics {
                diagnostics: vec![CellValueDiagnostic {
                    code: CellValueErrorCode::DuplicateField,
                    message: format!("duplicate field `{name}`"),
                }],
            });
        }
        if value_text == "_" {
            continue;
        }
        if value_text.is_empty() {
            return Err(syntax(format!("field `{name}` has an empty value")));
        }
        out.insert(
            name.to_string(),
            parse_value(schema, &field.ty, value_text, ValueContext::Nested)?,
        );
    }
    Ok(object_value(actual_type, out))
}

fn parse_positional_object(
    schema: &CftContainer,
    actual_type: Option<String>,
    fields: &[FieldMeta],
    parts: &[&str],
) -> Result<CfdInputValue, CellValueDiagnostics> {
    if parts.len() > fields.len() {
        return Err(syntax("too many positional object fields"));
    }

    let mut out = BTreeMap::new();
    for (field, part) in fields.iter().zip(parts) {
        let part = part.trim();
        if part == "_" {
            continue;
        }
        if part.is_empty() {
            return Err(syntax("positional object field has an empty value"));
        }
        out.insert(
            field.name.clone(),
            parse_value(schema, &field.ty, part, ValueContext::Nested)?,
        );
    }
    Ok(object_value(actual_type, out))
}

fn object_value(
    actual_type: Option<String>,
    fields: impl IntoIterator<Item = (String, CfdInputValue)>,
) -> CfdInputValue {
    if let Some(actual_type) = actual_type {
        CfdInputValue::object(actual_type, fields)
    } else {
        CfdInputValue::object_with_declared_type(fields)
    }
}

#[derive(Debug, Clone)]
struct ObjectContent<'a> {
    actual_type: Option<String>,
    content: &'a str,
    has_boundary: bool,
}

fn object_content(text: &str) -> ObjectContent<'_> {
    let text = text.trim();
    if let Some(inner) = strip_outer_pair(text, '{', '}') {
        return ObjectContent {
            actual_type: None,
            content: inner,
            has_boundary: true,
        };
    }

    if let Some(open) = find_marker_open_brace(text) {
        let actual_type = text[..open].trim();
        if is_type_marker_name(actual_type) {
            if let Some(inner) = strip_outer_pair(&text[open..], '{', '}') {
                return ObjectContent {
                    actual_type: Some(actual_type.to_string()),
                    content: inner,
                    has_boundary: true,
                };
            }
        }
    }

    ObjectContent {
        actual_type: None,
        content: text,
        has_boundary: false,
    }
}

fn is_type_marker_name(text: &str) -> bool {
    let mut chars = text.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first == '_' || is_xid_start(first)) && chars.all(|ch| ch == '_' || is_xid_continue(ch))
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

fn parse_string(text: &str) -> Result<String, CellValueDiagnostics> {
    let text = text.trim();
    if !text.starts_with('"') {
        if string_needs_quotes(text) {
            return Err(CellValueDiagnostics {
                diagnostics: vec![CellValueDiagnostic {
                    code: CellValueErrorCode::StringNeedsQuotes,
                    message: "string value must be quoted".to_string(),
                }],
            });
        }
        return Ok(text.to_string());
    }
    if !text.ends_with('"') || text.len() == 1 {
        return Err(syntax("unterminated string"));
    }
    let mut out = String::new();
    let mut escaped = false;
    for ch in text[1..text.len() - 1].chars() {
        if escaped {
            match ch {
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                other => {
                    return Err(syntax(format!("unsupported string escape `\\{other}`")));
                }
            }
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '"' {
            return Err(syntax("unescaped quote in string"));
        } else {
            out.push(ch);
        }
    }
    if escaped {
        return Err(syntax("unterminated string escape"));
    }
    Ok(out)
}

fn string_needs_quotes(text: &str) -> bool {
    text.is_empty()
        || matches!(text, "_" | "null")
        || text
            .chars()
            .any(|ch| matches!(ch, ',' | '|' | ':' | '{' | '}' | '[' | ']'))
}
