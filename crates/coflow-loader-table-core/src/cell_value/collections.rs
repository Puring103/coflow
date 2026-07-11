use coflow_cft::CompiledSchema;
use coflow_data_model::{CfdInputDictKey, CfdInputValue};

use super::diagnostics::{missing_boundary, syntax, type_mismatch, CellValueDiagnostics};
use super::scan::{find_top_level_char, split_top_level, strip_outer_pair};
use super::strings::parse_string;
use super::types::CellType;
use super::{parse_enum, parse_value, ValueContext};

pub(super) fn parse_array(
    schema: &CompiledSchema,
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

pub(super) fn parse_dict(
    schema: &CompiledSchema,
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
    schema: &CompiledSchema,
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
