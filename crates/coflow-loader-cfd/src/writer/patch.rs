use coflow_api::{DiagnosticSet, WriteCellRequest, WriteFieldPathSegment};
use coflow_cfd::ast::{CfdBlockEntry, CfdRecord as AstRecord, CfdValue as AstValue};
use coflow_cfd::CfdAst;
use coflow_cft::CftSchema;
use coflow_cft::Span;
use coflow_data_model::CfdValue;
use std::collections::BTreeMap;

use super::diag;
use super::render::serialize_value_for_type;
use super::schema_nav::type_after_field_segment;
use super::target::{locate_target, WriteTarget};

pub(super) fn apply_patch(
    source: &str,
    ast: &CfdAst,
    request: &WriteCellRequest<'_>,
) -> Result<String, DiagnosticSet> {
    validate_value(request.new_value)?;
    let record = find_record(ast, request.actual_type, request.record_key).ok_or_else(|| {
        DiagnosticSet::one(diag(
            "CFD-WRITE",
            format!(
                "record `{}.{}` not found in AST",
                request.actual_type, request.record_key
            ),
        ))
    })?;
    if request.field_path.is_empty() {
        return Err(DiagnosticSet::one(diag(
            "CFD-WRITE",
            "field_path must not be empty",
        )));
    }
    let WriteFieldPathSegment::Field(top_field) = &request.field_path[0] else {
        return Err(DiagnosticSet::one(diag(
            "CFD-WRITE",
            "top-level path must start with a field name",
        )));
    };

    match locate_target(
        request.schema,
        request.actual_type,
        record,
        request.field_path,
    )? {
        WriteTarget::Replace { span, ty } => {
            if span.start > source.len() || span.end > source.len() || span.start > span.end {
                return Err(DiagnosticSet::one(diag(
                    "CFD-WRITE",
                    format!(
                        "span [{}, {}) is out of bounds for source of length {}",
                        span.start,
                        span.end,
                        source.len()
                    ),
                )));
            }
            let fragment =
                serialize_value_for_type(request.new_value, Some(request.schema), Some(&ty), 1);
            Ok(format!(
                "{}{}{}",
                &source[..span.start],
                fragment,
                &source[span.end..]
            ))
        }
        WriteTarget::InsertTopLevel { ty } => {
            let block_end = record.span.end.min(source.len());
            let insert_pos = find_closing_brace(source, block_end)?;
            let fragment = format!(
                "  {top_field}: {},\n",
                serialize_value_for_type(request.new_value, Some(request.schema), Some(&ty), 2)
            );
            Ok(format!(
                "{}{}{}",
                &source[..insert_pos],
                fragment,
                &source[insert_pos..]
            ))
        }
        WriteTarget::InsertNested {
            block_span,
            depth,
            field_name,
            ty,
        } => {
            let block_end = block_span.end.min(source.len());
            let insert_pos = find_closing_brace(source, block_end)?;
            let indent = "  ".repeat(depth + 1);
            let outer = "  ".repeat(depth);
            let fragment = format!(
                "{indent}{field_name}: {},\n{outer}",
                serialize_value_for_type(
                    request.new_value,
                    Some(request.schema),
                    Some(&ty),
                    depth + 2
                )
            );
            Ok(format!(
                "{}{}{}",
                &source[..insert_pos],
                fragment,
                &source[insert_pos..]
            ))
        }
    }
}

pub(super) fn find_record<'a>(
    ast: &'a CfdAst,
    actual_type: &str,
    key: &str,
) -> Option<&'a AstRecord> {
    ast.records
        .iter()
        .find(|record| record.type_name == actual_type && record.key == key)
}

fn validate_value(v: &CfdValue) -> Result<(), DiagnosticSet> {
    match v {
        CfdValue::Object(record) => {
            for v in record.fields.values() {
                validate_value(v)?;
            }
            Ok(())
        }
        CfdValue::Array(items) => {
            for v in items {
                validate_value(v)?;
            }
            Ok(())
        }
        CfdValue::Dict(entries) => {
            for (_, v) in entries {
                validate_value(v)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

pub(super) fn validate_values<'a>(
    values: impl IntoIterator<Item = &'a CfdValue>,
) -> Result<(), DiagnosticSet> {
    for value in values {
        validate_value(value)?;
    }
    Ok(())
}

pub(super) fn validate_record_key(key: &str) -> Result<(), DiagnosticSet> {
    if key.trim().is_empty() {
        return Err(DiagnosticSet::one(diag(
            "CFD-WRITE",
            "record key must not be empty",
        )));
    }
    if let Some(reason) = coflow_cft::record_key_ident_error(key) {
        return Err(DiagnosticSet::one(diag(
            "CFD-WRITE",
            format!("record key `{key}` is invalid: {reason}"),
        )));
    }
    Ok(())
}

pub(super) fn serialize_record(
    schema: &CftSchema,
    key: &str,
    actual_type: &str,
    fields: &BTreeMap<String, CfdValue>,
) -> String {
    let mut out = format!("{key}: {actual_type} {{\n");
    for (name, value) in fields {
        out.push_str("  ");
        out.push_str(name);
        out.push_str(": ");
        let ty = type_after_field_segment(schema, actual_type, name);
        out.push_str(&serialize_value_for_type(
            value,
            Some(schema),
            ty.as_ref(),
            2,
        ));
        out.push_str(",\n");
    }
    out.push_str("}\n");
    out
}

pub(super) fn append_record_source(source: &str, fragment: &str) -> String {
    if source.trim().is_empty() {
        return fragment.to_string();
    }
    let mut out = source.trim_end().to_string();
    out.push_str("\n\n");
    out.push_str(fragment);
    out
}

pub(super) fn delete_record_span(source: &str, span: Span) -> Span {
    let mut start = span.start.min(source.len());
    let end = span.end.min(source.len());
    while start > 0 {
        let Some(prev) = source[..start].chars().next_back() else {
            break;
        };
        if prev == '\n' || prev == '\r' {
            start -= prev.len_utf8();
            continue;
        }
        break;
    }
    Span::new(start, end)
}

fn find_closing_brace(source: &str, near: usize) -> Result<usize, DiagnosticSet> {
    let end = near.min(source.len());
    let bytes = source.as_bytes();
    for i in (0..end).rev() {
        if bytes[i] == b'}' {
            return Ok(i);
        }
    }
    Err(DiagnosticSet::one(diag(
        "CFD-WRITE",
        "closing brace not found",
    )))
}

pub(super) fn collect_spread_ref_key_spans(
    entries: &[CfdBlockEntry],
    old_key: &str,
    out: &mut Vec<Span>,
) {
    for entry in entries {
        if let CfdBlockEntry::Spread(value, _) = entry {
            collect_ref_key_spans_in_value(value, old_key, out);
        }
    }
}

fn collect_ref_key_spans_in_value(value: &AstValue, old_key: &str, out: &mut Vec<Span>) {
    match value {
        AstValue::Ref(reference) => {
            if reference.key.0 == old_key {
                out.push(reference.key.1);
            }
        }
        AstValue::Array(items, _) => {
            for item in items {
                collect_ref_key_spans_in_value(item, old_key, out);
            }
        }
        AstValue::Spread(inner, _) => {
            collect_ref_key_spans_in_value(inner, old_key, out);
        }
        AstValue::Block(_)
        | AstValue::Scalar(_, _)
        | AstValue::QuotedString(_, _)
        | AstValue::Null(_) => {}
    }
}

pub(super) fn replace_spans(
    source: &str,
    replacements: &[(Span, String)],
) -> Result<String, DiagnosticSet> {
    let mut out = source.to_string();
    let mut sorted = replacements.to_vec();
    sorted.sort_by_key(|(span, _)| span.start);
    for (span, _) in &sorted {
        if span.start > source.len() || span.end > source.len() || span.start > span.end {
            return Err(DiagnosticSet::one(diag(
                "CFD-WRITE",
                format!(
                    "span [{}, {}) is out of bounds for source of length {}",
                    span.start,
                    span.end,
                    source.len()
                ),
            )));
        }
    }
    sorted.dedup_by_key(|(span, _)| (span.start, span.end));
    for (span, replacement) in sorted.into_iter().rev() {
        out.replace_range(span.start..span.end, &replacement);
    }
    Ok(out)
}
