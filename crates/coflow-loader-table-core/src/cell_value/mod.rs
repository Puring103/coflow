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

use coflow_cft::{record_key_ident_error, CftContainer, CftSchemaField};
use coflow_data_model::{CfdDictKey, CfdEnumValue, CfdInputDictKey, CfdInputValue, CfdValue};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use unicode_ident::{is_xid_continue, is_xid_start};

#[derive(Debug, Clone, PartialEq)]
pub enum ParsedCell {
    Omitted,
    Value(CfdInputValue),
}

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellValueDiagnostics {
    pub diagnostics: Vec<CellValueDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellValueDiagnostic {
    pub code: CellValueErrorCode,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CellValueErrorCode {
    Syntax,
    InvalidDeclaredType,
    UnknownType,
    UnknownField,
    DuplicateField,
    MissingBoundary,
    TypeMismatch,
    ObjectTypeMismatch,
    AbstractObjectType,
    InvalidEnumVariant,
    MixedObjectStyle,
    StringNeedsQuotes,
    ReferenceNeedsMarker,
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

/// Renders a runtime value into the same table-cell text grammar accepted by
/// [`parse_cell`].
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

/// Rewrites reference tokens inside one table-cell value string.
///
/// This intentionally scans the cell grammar instead of doing a blind
/// replace: quoted strings are skipped, and direct references only match
/// complete `&old` tokens without touching unrelated text.
#[must_use]
pub fn rewrite_record_reference_text(
    text: &str,
    target_type_names: &[String],
    old_key: &str,
    new_key: &str,
    rewrite_direct_refs: bool,
) -> Option<String> {
    let _ = target_type_names;
    let mut replacements = Vec::<(usize, usize)>::new();
    let mut index = 0;
    let mut in_string = false;
    let mut escaped = false;

    while index < text.len() {
        let Some(ch) = text[index..].chars().next() else {
            break;
        };
        if in_string {
            index += ch.len_utf8();
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        if ch == '"' {
            in_string = true;
            index += ch.len_utf8();
            continue;
        }
        if ch == '&' && rewrite_direct_refs {
            let key_start = index + ch.len_utf8();
            let key_end = scan_ref_name_end(text, key_start);
            if key_start < key_end && &text[key_start..key_end] == old_key {
                replacements.push((key_start, key_end));
                index = key_end;
                continue;
            }
        }
        index += ch.len_utf8();
    }

    if replacements.is_empty() {
        return None;
    }
    let mut out = text.to_string();
    replacements.sort_unstable();
    replacements.dedup();
    for (start, end) in replacements.into_iter().rev() {
        out.replace_range(start..end, new_key);
    }
    Some(out)
}

fn scan_ref_name_end(text: &str, start: usize) -> usize {
    let mut end = start;
    while end < text.len() {
        let Some(ch) = text[end..].chars().next() else {
            break;
        };
        if ch.is_whitespace()
            || matches!(
                ch,
                '.' | '[' | ']' | ',' | ';' | '}' | ')' | ':' | '@' | '&' | '|' | '{'
            )
        {
            break;
        }
        end += ch.len_utf8();
    }
    end
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

fn render_object(record: &coflow_data_model::CfdRecord) -> Result<String, CellRenderError> {
    let mut out = String::new();
    if !record.actual_type.is_empty() {
        out.push_str(&record.actual_type);
    }
    out.push('{');
    for (idx, (field, value)) in record.fields.iter().enumerate() {
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

fn render_string(value: &str) -> String {
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

#[derive(Debug, Clone, PartialEq, Eq)]
enum CellType {
    Int,
    Float,
    Bool,
    String,
    Type(String),
    Ref(String),
    Enum(String),
    Array(Box<CellType>),
    Dict(Box<CellType>, Box<CellType>),
    Nullable(Box<CellType>),
}

impl CellType {
    fn parse(schema: &CftContainer, text: &str) -> Result<Self, CellValueDiagnostics> {
        let mut parser = TypeParser::new(schema, text);
        let ty = parser.parse_type()?;
        parser.skip_ws();
        if parser.is_eof() {
            Ok(ty)
        } else {
            Err(invalid_declared_type("unexpected text after type"))
        }
    }

    fn display(&self) -> String {
        match self {
            Self::Int => "int".to_string(),
            Self::Float => "float".to_string(),
            Self::Bool => "bool".to_string(),
            Self::String => "string".to_string(),
            Self::Type(name) | Self::Enum(name) => name.clone(),
            Self::Ref(name) => format!("&{name}"),
            Self::Array(inner) => format!("[{}]", inner.display()),
            Self::Dict(key, value) => format!("{{{}: {}}}", key.display(), value.display()),
            Self::Nullable(inner) => format!("{}?", inner.display()),
        }
    }
}

struct TypeParser<'a> {
    schema: &'a CftContainer,
    text: &'a str,
    pos: usize,
}

impl<'a> TypeParser<'a> {
    fn new(schema: &'a CftContainer, text: &'a str) -> Self {
        Self {
            schema,
            text,
            pos: 0,
        }
    }

    fn parse_type(&mut self) -> Result<CellType, CellValueDiagnostics> {
        self.skip_ws();
        let mut ty = self.parse_primary()?;
        self.skip_ws();
        while self.eat('?') {
            ty = CellType::Nullable(Box::new(ty));
            self.skip_ws();
        }
        Ok(ty)
    }

    fn parse_primary(&mut self) -> Result<CellType, CellValueDiagnostics> {
        self.skip_ws();
        if self.eat('&') {
            let name = self.parse_name();
            if name.is_empty() {
                return Err(invalid_declared_type(
                    "reference type is missing target type",
                ));
            }
            if !self.schema.has_type(&name) {
                return Err(invalid_declared_type(format!(
                    "reference target `{name}` is not an object type"
                )));
            }
            return Ok(CellType::Ref(name));
        }
        if self.eat('[') {
            let inner = self.parse_type()?;
            self.skip_ws();
            if !self.eat(']') {
                return Err(invalid_declared_type("array type is missing `]`"));
            }
            return Ok(CellType::Array(Box::new(inner)));
        }
        if self.eat('{') {
            let key = self.parse_type()?;
            self.skip_ws();
            if !self.eat(':') {
                return Err(invalid_declared_type("dict type is missing `:`"));
            }
            let value = self.parse_type()?;
            self.skip_ws();
            if !self.eat('}') {
                return Err(invalid_declared_type("dict type is missing `}`"));
            }
            return Ok(CellType::Dict(Box::new(key), Box::new(value)));
        }

        let name = self.parse_name();
        if name.is_empty() {
            return Err(invalid_declared_type("expected type name"));
        }
        Ok(match name.as_str() {
            "int" => CellType::Int,
            "float" => CellType::Float,
            "bool" => CellType::Bool,
            "string" => CellType::String,
            other if self.schema.has_enum(other) => CellType::Enum(other.to_string()),
            other => CellType::Type(other.to_string()),
        })
    }

    fn parse_name(&mut self) -> String {
        self.skip_ws();
        let start = self.pos;
        while let Some(ch) = self.peek() {
            if matches!(
                ch,
                '[' | ']' | '{' | '}' | ':' | '?' | ' ' | '\t' | '\r' | '\n'
            ) {
                break;
            }
            self.pos += ch.len_utf8();
        }
        self.text[start..self.pos].to_string()
    }

    fn skip_ws(&mut self) {
        while let Some(ch) = self.peek() {
            if ch.is_whitespace() {
                self.pos += ch.len_utf8();
            } else {
                break;
            }
        }
    }

    fn eat(&mut self, expected: char) -> bool {
        if self.peek() == Some(expected) {
            self.pos += expected.len_utf8();
            true
        } else {
            false
        }
    }

    fn peek(&self) -> Option<char> {
        self.text[self.pos..].chars().next()
    }

    fn is_eof(&self) -> bool {
        self.pos == self.text.len()
    }
}

#[derive(Debug, Clone)]
struct FieldMeta {
    name: String,
    ty: CellType,
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

fn full_fields(
    schema: &CftContainer,
    type_name: &str,
) -> Result<Vec<FieldMeta>, CellValueDiagnostics> {
    let Some(schema_type) = schema.resolve_type(type_name) else {
        return Err(CellValueDiagnostics {
            diagnostics: vec![CellValueDiagnostic {
                code: CellValueErrorCode::UnknownType,
                message: format!("unknown type `{type_name}`"),
            }],
        });
    };
    schema_type
        .all_fields
        .iter()
        .map(|field| field_meta(schema, field))
        .collect()
}

fn field_meta(
    schema: &CftContainer,
    field: &CftSchemaField,
) -> Result<FieldMeta, CellValueDiagnostics> {
    Ok(FieldMeta {
        name: field.name.clone(),
        ty: CellType::parse(schema, &field.ty)?,
    })
}

fn split_top_level(input: &str, delimiter: char) -> Result<Vec<&str>, CellValueDiagnostics> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut state = ScanState::default();
    for (index, ch) in input.char_indices() {
        state.step(ch)?;
        if state.is_top_level() && ch == delimiter {
            parts.push(input[start..index].trim());
            start = index + ch.len_utf8();
        }
    }
    state.finish()?;
    parts.push(input[start..].trim());
    Ok(parts)
}

fn find_top_level_char(input: &str, target: char) -> Result<Option<usize>, CellValueDiagnostics> {
    let mut state = ScanState::default();
    for (index, ch) in input.char_indices() {
        state.step(ch)?;
        if state.is_top_level() && ch == target {
            return Ok(Some(index));
        }
    }
    state.finish()?;
    Ok(None)
}

fn strip_outer_pair(input: &str, open: char, close: char) -> Option<&str> {
    let input = input.trim();
    if !input.starts_with(open) {
        return None;
    }
    let mut state = ScanState::default();
    for (index, ch) in input.char_indices() {
        if state.step(ch).is_err() {
            return None;
        }
        if ch == close && state.is_top_level() {
            let end = index + ch.len_utf8();
            return (end == input.len()).then_some(&input[open.len_utf8()..index]);
        }
    }
    None
}

fn find_marker_open_brace(input: &str) -> Option<usize> {
    let mut state = ScanState::default();
    for (index, ch) in input.char_indices() {
        if ch == '{' && state.is_top_level() {
            return (index > 0).then_some(index);
        }
        if state.step(ch).is_err() {
            return None;
        }
    }
    None
}

#[derive(Debug, Default)]
struct ScanState {
    stack: Vec<char>,
    in_string: bool,
    escaped: bool,
}

impl ScanState {
    fn step(&mut self, ch: char) -> Result<(), CellValueDiagnostics> {
        if self.in_string {
            if self.escaped {
                self.escaped = false;
            } else if ch == '\\' {
                self.escaped = true;
            } else if ch == '"' {
                self.in_string = false;
            }
            return Ok(());
        }

        match ch {
            '"' => self.in_string = true,
            '{' => self.stack.push('}'),
            '[' => self.stack.push(']'),
            '}' | ']' if self.stack.pop() != Some(ch) => {
                return Err(syntax("mismatched brackets"));
            }
            _ => {}
        }
        Ok(())
    }

    fn is_top_level(&self) -> bool {
        !self.in_string && self.stack.is_empty()
    }

    fn finish(self) -> Result<(), CellValueDiagnostics> {
        if self.in_string {
            return Err(syntax("unterminated string"));
        }
        if !self.stack.is_empty() {
            return Err(syntax("unclosed brackets"));
        }
        Ok(())
    }
}

fn syntax(message: impl Into<String>) -> CellValueDiagnostics {
    CellValueDiagnostics {
        diagnostics: vec![CellValueDiagnostic {
            code: CellValueErrorCode::Syntax,
            message: message.into(),
        }],
    }
}

fn invalid_declared_type(message: impl Into<String>) -> CellValueDiagnostics {
    CellValueDiagnostics {
        diagnostics: vec![CellValueDiagnostic {
            code: CellValueErrorCode::InvalidDeclaredType,
            message: message.into(),
        }],
    }
}

fn missing_boundary(message: impl Into<String>) -> CellValueDiagnostics {
    CellValueDiagnostics {
        diagnostics: vec![CellValueDiagnostic {
            code: CellValueErrorCode::MissingBoundary,
            message: message.into(),
        }],
    }
}

fn type_mismatch(expected: &str) -> CellValueDiagnostics {
    CellValueDiagnostics {
        diagnostics: vec![CellValueDiagnostic {
            code: CellValueErrorCode::TypeMismatch,
            message: format!("expected {expected}"),
        }],
    }
}

fn reference_needs_marker(text: &str) -> CellValueDiagnostics {
    CellValueDiagnostics {
        diagnostics: vec![CellValueDiagnostic {
            code: CellValueErrorCode::ReferenceNeedsMarker,
            message: format!(
                "record reference `{text}` must be written as `&{text}` in a reference-typed field"
            ),
        }],
    }
}
