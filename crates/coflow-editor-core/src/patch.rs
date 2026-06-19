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
    let block_end = record.span.end;
    // Walk back from block_end to find the closing }
    let insert_pos = find_closing_brace(source, block_end)?;
    let fragment = format!("  {}: {},\n", field_name, serialize_value(value));
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
        FieldValue::Object {
            actual_type,
            fields,
        } => {
            let body: String = fields
                .iter()
                .map(|f| format!("  {}: {},\n", f.name, serialize_value(&f.value)))
                .collect();
            format!("{actual_type} {{\n{body}}}")
        }
        FieldValue::Array { items } => {
            let elems: Vec<String> = items.iter().map(serialize_value).collect();
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
                    format!("{k}: {}", serialize_value(&e.value))
                })
                .collect();
            format!("{{{}}}", pairs.join(", "))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
