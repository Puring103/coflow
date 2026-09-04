use crate::api::{DiagnosticSet, WriteCellRequest, WriteFieldPathSegment};
use crate::data_model::CfdValue;
use coflow_language::CftSchema;
use coflow_language::Span;
use coflow_language::cfd::CfdAst;
use coflow_language::cfd::ast::CfdRecord as AstRecord;
use std::collections::BTreeMap;

use super::CFD_INDENT;
use super::diag;
use super::render::serialize_value_for_type;
use super::schema_nav::type_after_field_segment;
use super::target::{WriteTarget, locate_target};

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
            let depth = replacement_serialization_depth(source, span.start, request.new_value);
            let fragment =
                serialize_value_for_type(request.new_value, Some(request.schema), Some(&ty), depth);
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
                "{CFD_INDENT}{top_field}: {},\n",
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
            let indent = CFD_INDENT.repeat(depth + 1);
            let outer = CFD_INDENT.repeat(depth);
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

fn replacement_serialization_depth(source: &str, offset: usize, value: &CfdValue) -> usize {
    let line_start = source[..offset.min(source.len())]
        .rfind('\n')
        .map_or(0, |index| index + 1);
    let line_indent = source[line_start..]
        .chars()
        .take_while(|character| *character == ' ')
        .count()
        / CFD_INDENT.len();
    line_indent + usize::from(value_starts_object(value))
}

fn value_starts_object(value: &CfdValue) -> bool {
    match value {
        CfdValue::Object(_) => true,
        CfdValue::OptionSome(inner) | CfdValue::ResultOk(inner) | CfdValue::ResultErr(inner) => {
            value_starts_object(inner)
        }
        _ => false,
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

pub(super) fn apply_unset_field_patch(
    schema: &CftSchema,
    source: &str,
    ast: &CfdAst,
    actual_type: &str,
    record_key: &str,
    field_path: &[WriteFieldPathSegment],
) -> Result<String, DiagnosticSet> {
    if field_path.is_empty() {
        return Err(DiagnosticSet::one(diag(
            "CFD-WRITE",
            "field_path must not be empty",
        )));
    }
    let record = find_record(ast, actual_type, record_key).ok_or_else(|| {
        DiagnosticSet::one(diag(
            "CFD-WRITE",
            format!("record `{actual_type}.{record_key}` not found in AST"),
        ))
    })?;
    let WriteTarget::Replace { span, .. } = locate_target(schema, actual_type, record, field_path)?
    else {
        return Err(DiagnosticSet::one(diag(
            "CFD-WRITE",
            "field_path must identify an existing field to unset",
        )));
    };
    let removed = removed_field_span(source, span);
    Ok(format!(
        "{}{}",
        source.get(..removed.start).ok_or_else(|| {
            DiagnosticSet::one(diag(
                "CFD-WRITE",
                "field span is outside the source document",
            ))
        })?,
        source.get(removed.end..).ok_or_else(|| {
            DiagnosticSet::one(diag(
                "CFD-WRITE",
                "field span is outside the source document",
            ))
        })?,
    ))
}

fn removed_field_span(source: &str, value_span: Span) -> Span {
    let mut start = value_span.start.min(source.len());
    while start > 0 {
        let Some(previous) = source[..start].chars().next_back() else {
            break;
        };
        if previous == '\n' {
            break;
        }
        start -= previous.len_utf8();
    }
    let mut end = value_span.end.min(source.len());
    end += source[end..]
        .chars()
        .take_while(|c| c.is_whitespace() && *c != '\n')
        .map(char::len_utf8)
        .sum::<usize>();
    if source[end..].starts_with(',') {
        end += 1;
        end += source[end..]
            .chars()
            .take_while(|c| c.is_whitespace() && *c != '\n')
            .map(char::len_utf8)
            .sum::<usize>();
    }
    if source[end..].starts_with("\r\n") {
        end += 2;
    } else if source[end..].starts_with('\n') {
        end += 1;
    } else {
        end = source.len();
    }
    Span::new(start, end)
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
    if let Some(reason) = coflow_language::record_key_ident_error(key) {
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
        out.push_str(CFD_INDENT);
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

pub(super) fn reorder_record_spans(
    source: &str,
    records: &[AstRecord],
    order: &[usize],
) -> Result<String, DiagnosticSet> {
    if order.len() != records.len() {
        return Err(DiagnosticSet::one(diag(
            "CFD-WRITE",
            "record reorder does not cover every document record",
        )));
    }
    let replacements = records
        .iter()
        .zip(order)
        .map(
            |(slot, source_index)| -> Result<(Span, String), DiagnosticSet> {
                let moved = records.get(*source_index).ok_or_else(|| {
                    DiagnosticSet::one(diag(
                        "CFD-WRITE",
                        "record reorder index is outside the document",
                    ))
                })?;
                let fragment = source
                    .get(moved.span.start..moved.span.end)
                    .ok_or_else(|| {
                        DiagnosticSet::one(diag(
                            "CFD-WRITE",
                            "record span is outside the source document",
                        ))
                    })?;
                let fragment = if record_container(slot) == record_container(moved) {
                    fragment.to_string()
                } else {
                    explicit_record_fragment(fragment, moved)?
                };
                Ok((slot.span, fragment))
            },
        )
        .collect::<Result<Vec<_>, _>>()?;
    replace_spans(source, &replacements)
}

fn record_container(record: &AstRecord) -> Option<usize> {
    record.group_type.as_ref().map(|(_, span)| span.start)
}

fn explicit_record_fragment(fragment: &str, record: &AstRecord) -> Result<String, DiagnosticSet> {
    if record.type_span.start >= record.span.start {
        return Ok(fragment.to_string());
    }
    let insert_at = record.key_span.end.saturating_sub(record.span.start);
    let Some((prefix, suffix)) = fragment.get(..insert_at).zip(fragment.get(insert_at..)) else {
        return Err(DiagnosticSet::one(diag(
            "CFD-WRITE",
            "record key span is outside the record fragment",
        )));
    };
    Ok(format!("{prefix}: {}{suffix}", record.type_name))
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
