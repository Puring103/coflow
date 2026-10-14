use crate::{CfdRecord, CfdValue, ReadOnlyProjectSession};
use super::{Failure, Result, query::{parse_path, value_at, compare_values}};
use std::cmp::Ordering;

pub fn matches(session: &ReadOnlyProjectSession, record: &CfdRecord, expression: &str) -> Result<bool> {
    evaluate(session, record, expression.trim(), 0)
}

fn evaluate(session: &ReadOnlyProjectSession, record: &CfdRecord, expression: &str, depth: usize) -> Result<bool> {
    if depth > 64 || expression.len() > 65536 { return Err(Failure::request("filter structural budget exceeded")); }
    for operator in ["||", "&&"] {
        if let Some((left, right)) = split(expression, operator) {
            // 两侧均做类型检查，不能由短路掩盖错误字段或错误常量。
            let left = evaluate(session, record, left.trim(), depth + 1)?;
            let right = evaluate(session, record, right.trim(), depth + 1)?;
            return Ok(if operator == "||" { left || right } else { left && right });
        }
    }
    if let Some(inner) = expression.strip_prefix('!') { return Ok(!evaluate(session, record, inner.trim(), depth + 1)?); }
    if expression.starts_with('(') && expression.ends_with(')') {
        return evaluate(session, record, &expression[1..expression.len()-1], depth + 1);
    }
    for operator in [" is none", " is some", " is missing", " is empty"] {
        if let Some(field) = expression.strip_suffix(operator) {
            super::query::validate_field(session, record.actual_type(), field.trim())?;
            let value = value_at(record, field.trim())?;
            return Ok(match operator {
                " is none" => matches!(value, Some(CfdValue::OptionNone)),
                " is some" => matches!(value, Some(CfdValue::OptionSome(_))),
                " is missing" => value.is_none(),
                _ => matches!(value, Some(CfdValue::String(v)) if v.is_empty())
                    || matches!(value, Some(CfdValue::Array(v)) if v.is_empty())
                    || matches!(value, Some(CfdValue::Dict(v)) if v.is_empty()),
            });
        }
    }
    for operator in ["==", "!=", ">=", "<=", ">", "<", " contains "] {
        if let Some((field, literal)) = split(expression, operator) {
            let field = field.trim();
            let path = parse_path(field)?;
            let expected = crate::write_rules::expected_type_for_cfd_path(session.schema(), record.actual_type(), &path, "CLI-FILTER", "CLI")?;
            let value = value_at(record, field)?;
            if operator == " contains " {
                return match (&expected,value) {
                    (coflow_language::cft::CftValueType::String,Some(CfdValue::String(text))) => Ok(text.contains(&serde_json::from_str::<String>(literal.trim())?)),
                    (coflow_language::cft::CftValueType::Array(inner),Some(CfdValue::Array(items))) => {
                        let item = super::values::expression(session.schema(),inner,literal.trim())?;
                        Ok(items.contains(&item))
                    }
                    _ => Err(Failure::request("contains requires a string or array field")),
                };
            }
            let other = super::values::expression(session.schema(),&expected,literal.trim())?;
            if !["==","!="].contains(&operator) && !matches!(other,CfdValue::Int(_)|CfdValue::Float(_)|CfdValue::String(_)|CfdValue::Enum(_)) {
                return Err(Failure::request("ordered comparison requires a number, enum or string"));
            }
            let order = compare_values(value, Some(&other));
            return Ok(match operator {
                "==" => value == Some(&other), "!=" => value != Some(&other),
                ">" => order == Ordering::Greater, "<" => order == Ordering::Less,
                ">=" => order != Ordering::Less, "<=" => order != Ordering::Greater, _ => false,
            });
        }
    }
    if expression == "true" { return Ok(true); }
    if expression == "false" { return Ok(false); }
    match value_at(record, expression)? {
        Some(CfdValue::Bool(value)) => Ok(*value),
        _ => Err(Failure::request("filter requires a boolean expression")),
    }
}

fn split<'a>(source: &'a str, needle: &str) -> Option<(&'a str, &'a str)> {
    let mut quoted = false; let mut escaped = false; let mut depth = 0usize;
    for (offset, ch) in source.char_indices() {
        if !quoted && depth == 0 && source[offset..].starts_with(needle) { return Some((&source[..offset], &source[offset+needle.len()..])); }
        if ch == '"' && !escaped { quoted = !quoted; }
        if !quoted { match ch { '(' | '[' | '{' => depth += 1, ')' | ']' | '}' => depth = depth.saturating_sub(1), _ => {} } }
        escaped = ch == '\\' && !escaped;
    }
    None
}
