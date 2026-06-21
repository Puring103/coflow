//! Span-patch writer: locate a field value in the CfdAst by path and replace
//! its byte span in the source text with a serialized CFD fragment.

use crate::types::{DictKey, FieldPathSegment, FieldValue};
use coflow_cfd::ast::{CfdBlockEntry, CfdRecord, CfdValue};
use coflow_cft::Span;

#[derive(Debug)]
pub struct PatchResult {
    pub new_source: String,
}

pub fn apply_patch(
    source: &str,
    ast: &coflow_cfd::CfdAst,
    record_key: &str,
    field_path: &[FieldPathSegment],
    new_value: &FieldValue,
) -> Result<PatchResult, String> {
    validate_value(new_value)?;
    let record = ast
        .records
        .iter()
        .find(|r| r.key == record_key)
        .ok_or_else(|| format!("record '{record_key}' not found in AST"))?;

    if field_path.is_empty() {
        return Err("field_path must not be empty".to_string());
    }

    let span = locate_span(record, field_path)?;
    if span.start > source.len() || span.end > source.len() || span.start > span.end {
        return Err(format!(
            "span [{}, {}) is out of bounds for source of length {}",
            span.start,
            span.end,
            source.len()
        ));
    }
    let fragment = serialize_value(new_value);
    let new_source = format!(
        "{}{}{}",
        &source[..span.start],
        fragment,
        &source[span.end..]
    );
    Ok(PatchResult { new_source })
}

/// Insert a new top-level field into a record (field does not exist yet).
pub fn insert_field(
    source: &str,
    ast: &coflow_cfd::CfdAst,
    record_key: &str,
    field_name: &str,
    value: &FieldValue,
) -> Result<PatchResult, String> {
    validate_value(value)?;
    let record = ast
        .records
        .iter()
        .find(|r| r.key == record_key)
        .ok_or_else(|| format!("record '{record_key}' not found"))?;

    let block_end = record.span.end.min(source.len());
    let insert_pos = find_closing_brace(source, block_end)?;
    let fragment = format!("  {}: {},\n", field_name, serialize_value_indented(value, 2));
    let new_source = format!(
        "{}{}{}",
        &source[..insert_pos],
        fragment,
        &source[insert_pos..]
    );
    Ok(PatchResult { new_source })
}

fn find_closing_brace(source: &str, near: usize) -> Result<usize, String> {
    let end = near.min(source.len());
    let bytes = source.as_bytes();
    for i in (0..end).rev() {
        if bytes[i] == b'}' {
            return Ok(i);
        }
    }
    Err("closing brace not found".to_string())
}

fn full_value_span(value: &CfdValue) -> Span {
    if let CfdValue::Block(b) = value {
        if let Some((_, tm_span)) = &b.type_marker {
            return Span::new(tm_span.start, b.span.end);
        }
    }
    value.span()
}

fn locate_span(record: &CfdRecord, path: &[FieldPathSegment]) -> Result<Span, String> {
    let first = &path[0];
    match first {
        FieldPathSegment::Field { name } => {
            let field = find_field_in_record(record, name)?;
            if path.len() == 1 {
                Ok(full_value_span(&field.value))
            } else {
                locate_span_in_value(&field.value, &path[1..])
            }
        }
        FieldPathSegment::Index { .. } => {
            Err("top-level path must start with a field name".to_string())
        }
    }
}

fn find_field_in_record<'a>(
    record: &'a CfdRecord,
    name: &str,
) -> Result<&'a coflow_cfd::CfdField, String> {
    record
        .fields
        .iter()
        .find(|f| f.name == name)
        .or_else(|| {
            record.entries.iter().find_map(|e| match e {
                CfdBlockEntry::Field(f) if f.name == name => Some(f),
                _ => None,
            })
        })
        .ok_or_else(|| format!("field '{name}' not found in record"))
}

fn locate_span_in_value(value: &CfdValue, path: &[FieldPathSegment]) -> Result<Span, String> {
    if path.is_empty() {
        return Ok(full_value_span(value));
    }
    match (&path[0], value) {
        (FieldPathSegment::Field { name }, CfdValue::Block(block)) => {
            let field = block
                .entries
                .iter()
                .find_map(|e| match e {
                    CfdBlockEntry::Field(f) if &f.name == name => Some(f),
                    _ => None,
                })
                .ok_or_else(|| format!("field '{name}' not found in block"))?;
            if path.len() == 1 {
                Ok(full_value_span(&field.value))
            } else {
                locate_span_in_value(&field.value, &path[1..])
            }
        }
        (FieldPathSegment::Index { i }, CfdValue::Array(items, _)) => {
            let item = items
                .get(*i)
                .ok_or_else(|| format!("index {i} out of bounds"))?;
            if path.len() == 1 {
                Ok(full_value_span(item))
            } else {
                locate_span_in_value(item, &path[1..])
            }
        }
        _ => Err(format!(
            "cannot navigate path segment {:?} in value",
            path[0]
        )),
    }
}

pub fn serialize_value(v: &FieldValue) -> String {
    serialize_value_indented(v, 1)
}

/// Reject values that would serialize to invalid CFD source.
/// Specifically: a Ref with an empty target_key (`&` or `@Type.`) breaks the
/// parser and erases the whole file's records on next reload.
fn validate_value(v: &FieldValue) -> Result<(), String> {
    match v {
        FieldValue::Ref { target_key, target_type, .. } => {
            if target_key.is_empty() {
                return Err(format!(
                    "cannot write empty reference (target_type={target_type:?}); pick a target key first"
                ));
            }
            Ok(())
        }
        FieldValue::Object { fields, .. } => {
            for f in fields {
                validate_value(&f.value)?;
            }
            Ok(())
        }
        FieldValue::Array { items } => {
            for i in items {
                validate_value(i)?;
            }
            Ok(())
        }
        FieldValue::Dict { entries } => {
            for e in entries {
                validate_value(&e.value)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn serialize_value_indented(v: &FieldValue, depth: usize) -> String {
    let indent = "  ".repeat(depth);
    let outer = "  ".repeat(depth.saturating_sub(1));
    match v {
        FieldValue::Null => "null".to_string(),
        FieldValue::Bool { v } => v.to_string(),
        FieldValue::Int { v } => v.to_string(),
        FieldValue::Float { v } => {
            let s = v.to_string();
            // CFD parser distinguishes int from float by the presence of '.'.
            if s.contains('.') || s.contains('e') || s.contains('E') {
                s
            } else {
                format!("{s}.0")
            }
        }
        FieldValue::Str { v } => format!("{v:?}"),
        FieldValue::Enum { variant, .. } => variant.clone(),
        FieldValue::Ref { target_key, target_type, .. } => {
            // Always emit the fully-qualified `@Type.key` form. The bare
            // `&key` shortcut is only valid when the field's schema type is
            // exactly Ref-to-T; for polymorphic fields (e.g. `Reward`,
            // a base type with subtypes) `&key` is rejected by the parser.
            // We don't have schema context at this layer, so qualify always.
            if target_type.is_empty() {
                format!("&{target_key}")
            } else {
                format!("@{target_type}.{target_key}")
            }
        }
        FieldValue::Object { actual_type, fields } => {
            let body: String = fields
                .iter()
                .map(|f| {
                    format!(
                        "{indent}{}: {},\n",
                        f.name,
                        serialize_value_indented(&f.value, depth + 1)
                    )
                })
                .collect();
            format!("{actual_type} {{\n{body}{outer}}}")
        }
        FieldValue::Array { items } => {
            let elems: Vec<String> = items
                .iter()
                .map(|i| serialize_value_indented(i, depth))
                .collect();
            format!("[{}]", elems.join(", "))
        }
        FieldValue::Dict { entries } => {
            let pairs: Vec<String> = entries
                .iter()
                .map(|e| {
                    let k = match &e.key {
                        DictKey::Str { v } => format!("{v:?}"),
                        DictKey::Int { v } => v.to_string(),
                        DictKey::Enum { variant, .. } => variant.clone(),
                    };
                    format!("{k}: {}", serialize_value_indented(&e.value, depth))
                })
                .collect();
            format!("{{{}}}", pairs.join(", "))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(src: &str) -> coflow_cfd::CfdAst {
        let (ast, diags) = coflow_cfd::parse_cfd(src);
        assert!(diags.is_empty(), "parse errors: {diags:?}");
        ast
    }

    #[test]
    fn apply_patch_replaces_scalar_field() {
        let src = "sword: Item {\n  count: 1,\n}\n";
        let ast = parse(src);
        let result = apply_patch(
            src,
            &ast,
            "sword",
            &[FieldPathSegment::Field { name: "count".to_string() }],
            &FieldValue::Int { v: 42 },
        )
        .unwrap();
        assert!(result.new_source.contains("count: 42"), "unexpected: {}", result.new_source);
        assert!(!result.new_source.contains("count: 1,"), "old value still present: {}", result.new_source);
    }

    #[test]
    fn apply_patch_replaces_string_field() {
        let src = "sword: Item {\n  name: \"OldName\",\n}\n";
        let ast = parse(src);
        let result = apply_patch(
            src,
            &ast,
            "sword",
            &[FieldPathSegment::Field { name: "name".to_string() }],
            &FieldValue::Str { v: "NewName".to_string() },
        )
        .unwrap();
        assert!(result.new_source.contains("\"NewName\""), "expected new string: {}", result.new_source);
        assert!(!result.new_source.contains("\"OldName\""), "old string still present: {}", result.new_source);
    }

    #[test]
    fn ref_serialization_qualifies_with_type() {
        let v = FieldValue::Ref {
            target_type: "Item".to_string(),
            target_key: "sword_fire".to_string(),
            target_file: Some("data/items.cfd".to_string()),
        };
        assert_eq!(serialize_value(&v), "@Item.sword_fire");
    }

    #[test]
    fn empty_ref_key_is_rejected() {
        let src = "sword: Item {\n  upgrade: @Item.iron_sword,\n}\n";
        let ast = parse(src);
        let err = apply_patch(
            src,
            &ast,
            "sword",
            &[FieldPathSegment::Field { name: "upgrade".to_string() }],
            &FieldValue::Ref {
                target_type: "Item".to_string(),
                target_key: "".to_string(),
                target_file: None,
            },
        )
        .unwrap_err();
        assert!(err.contains("empty reference"), "expected guard message, got: {err}");
    }

    #[test]
    fn float_serialization_preserves_decimal() {
        assert_eq!(serialize_value(&FieldValue::Float { v: 1.0 }), "1.0");
        assert_eq!(serialize_value(&FieldValue::Float { v: 1.5 }), "1.5");
    }
}
