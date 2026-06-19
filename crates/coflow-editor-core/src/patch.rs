// Span-patch writer: locate a field value in the CfdAst by path, replace its
// byte span in the source text with a serialized CFD fragment.

use crate::types::{DictKey, FieldPathSegment, FieldValue};
use coflow_cfd::ast::{CfdBlockEntry, CfdRecord, CfdValue};
use coflow_cft::Span;

#[derive(Debug)]
pub struct PatchResult {
    pub new_source: String,
}

/// Apply a field-value patch.
/// field_path is relative to the record (empty = replace entire record value? no —
/// for now field_path must have at least one segment pointing to a field name).
pub fn apply_patch(
    source: &str,
    ast: &coflow_cfd::CfdAst,
    record_key: &str,
    field_path: &[FieldPathSegment],
    new_value: &FieldValue,
) -> Result<PatchResult, String> {
    // Find the record in AST
    let record = ast
        .records
        .iter()
        .find(|r| r.key == record_key)
        .ok_or_else(|| format!("record '{record_key}' not found in AST"))?;

    // Navigate field_path
    if field_path.is_empty() {
        return Err("field_path must not be empty".to_string());
    }

    let span = locate_span(record, field_path)?;
    if span.start > source.len() || span.end > source.len() || span.start > span.end {
        return Err(format!(
            "span [{}, {}) is out of bounds for source of length {}",
            span.start, span.end, source.len()
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

/// Insert a new field into a record (field does not exist yet).
/// Inserts before the closing } of the record's top-level block.
pub fn insert_field(
    source: &str,
    ast: &coflow_cfd::CfdAst,
    record_key: &str,
    field_name: &str,
    value: &FieldValue,
) -> Result<PatchResult, String> {
    let record = ast
        .records
        .iter()
        .find(|r| r.key == record_key)
        .ok_or_else(|| format!("record '{record_key}' not found"))?;

    // The record's block ends at record.span.end — find the last } character
    let block_end = record.span.end.min(source.len());
    // Walk back from block_end to find the closing }
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
    // Search backwards for '}'
    for i in (0..end).rev() {
        if bytes[i] == b'}' {
            return Ok(i);
        }
    }
    Err("closing brace not found".to_string())
}

fn locate_span(record: &CfdRecord, path: &[FieldPathSegment]) -> Result<Span, String> {
    // Navigate through CfdRecord fields/blocks following field_path
    let first = &path[0];
    match first {
        FieldPathSegment::Field { name } => {
            let field = find_field_in_record(record, name)?;
            if path.len() == 1 {
                Ok(field.value.span())
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
        return Ok(value.span());
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
                Ok(field.value.span())
            } else {
                locate_span_in_value(&field.value, &path[1..])
            }
        }
        (FieldPathSegment::Index { i }, CfdValue::Array(items, _)) => {
            let item = items
                .get(*i)
                .ok_or_else(|| format!("index {i} out of bounds"))?;
            if path.len() == 1 {
                Ok(item.span())
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

fn serialize_value_indented(v: &FieldValue, depth: usize) -> String {
    let indent = "  ".repeat(depth);
    let outer = "  ".repeat(depth.saturating_sub(1));
    match v {
        FieldValue::Null => "null".to_string(),
        FieldValue::Bool { v } => v.to_string(),
        FieldValue::Int { v } => (*v as i64).to_string(),
        FieldValue::Float { v } => {
            let s = v.to_string();
            // Rust Display for f64 omits the decimal point for whole numbers (1.0 → "1").
            // CFD parser distinguishes int from float by the presence of '.', so ensure it.
            if s.contains('.') || s.contains('e') || s.contains('E') { s } else { format!("{s}.0") }
        }
        FieldValue::Str { v } => format!("{v:?}"), // Rust debug format = JSON string escaping
        FieldValue::Enum { variant, .. } => variant.clone(),
        FieldValue::Ref { target_key, .. } => {
            // Always serialize as &key — the CFD loader resolves by key across all files.
            // @Type.key syntax is a reader convenience; we don't need to reproduce it.
            format!("&{target_key}")
        }
        FieldValue::Object { actual_type, fields } => {
            let body: String = fields
                .iter()
                .map(|f| format!("{indent}{}: {},\n", f.name, serialize_value_indented(&f.value, depth + 1)))
                .collect();
            format!("{actual_type} {{\n{body}{outer}}}")
        }
        FieldValue::Array { items } => {
            let elems: Vec<String> = items.iter().map(|i| serialize_value_indented(i, depth)).collect();
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
        // Standalone record: key: TypeName { ... }
        let src = "sword: Item {\n  count: 1,\n}\n";
        let ast = parse(src);
        let result = apply_patch(
            src,
            &ast,
            "sword",
            &[FieldPathSegment::Field { name: "count".to_string() }],
            &FieldValue::Int { v: 42.0 },
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
    fn apply_patch_record_not_found_errors() {
        let src = "sword: Item {\n  count: 1,\n}\n";
        let ast = parse(src);
        let err = apply_patch(
            src,
            &ast,
            "shield",
            &[FieldPathSegment::Field { name: "count".to_string() }],
            &FieldValue::Int { v: 1.0 },
        )
        .unwrap_err();
        assert!(err.contains("shield"), "expected record name in error: {err}");
    }

    #[test]
    fn insert_field_adds_to_empty_record() {
        // Grouped record: TypeName { key { } }
        let src = "Item {\n  sword {\n  }\n}\n";
        let ast = parse(src);
        let result = insert_field(src, &ast, "sword", "count", &FieldValue::Int { v: 5.0 }).unwrap();
        assert!(result.new_source.contains("count: 5"), "expected inserted field: {}", result.new_source);
        assert!(result.new_source.contains("sword"), "record key missing: {}", result.new_source);
    }

    #[test]
    fn insert_field_adds_to_nonempty_record() {
        let src = "sword: Item {\n  name: \"Sword\",\n}\n";
        let ast = parse(src);
        let result = insert_field(src, &ast, "sword", "count", &FieldValue::Int { v: 3.0 }).unwrap();
        assert!(result.new_source.contains("count: 3"), "expected inserted field: {}", result.new_source);
        assert!(result.new_source.contains("name: \"Sword\""), "existing field missing: {}", result.new_source);
    }

    #[test]
    fn float_serialization_preserves_decimal() {
        assert_eq!(serialize_value(&FieldValue::Float { v: 1.0 }), "1.0");
        assert_eq!(serialize_value(&FieldValue::Float { v: 1.5 }), "1.5");
        assert_eq!(serialize_value(&FieldValue::Float { v: -2.0 }), "-2.0");
        assert_eq!(serialize_value(&FieldValue::Float { v: 1e10 }), "10000000000.0");
        // Values already containing exponent notation pass through
        let big = 1e20f64;
        let s = serialize_value(&FieldValue::Float { v: big });
        assert!(s.contains('.') || s.contains('e') || s.contains('E'), "expected decimal or exponent in {s}");
    }

    #[test]
    fn ref_serialization_uses_ampersand() {
        let v = FieldValue::Ref {
            target_type: "Item".to_string(),
            target_key: "sword_fire".to_string(),
            target_file: Some("data/items.cfd".to_string()),
        };
        assert_eq!(serialize_value(&v), "&sword_fire");
    }

    #[test]
    fn nested_object_serialization_indentation() {
        use crate::types::FieldCell;
        let v = FieldValue::Object {
            actual_type: "Outer".to_string(),
            fields: vec![
                FieldCell {
                    name: "inner".to_string(),
                    value: FieldValue::Object {
                        actual_type: "Inner".to_string(),
                        fields: vec![FieldCell { name: "x".to_string(), value: FieldValue::Int { v: 1.0 } }],
                    },
                },
            ],
        };
        let s = serialize_value(&v);
        // Inner object fields should be indented further than outer fields
        assert!(s.contains("  inner:"), "outer field should be at 2-space indent:\n{s}");
        assert!(s.contains("    x:"), "inner field should be at 4-space indent:\n{s}");
    }
}
