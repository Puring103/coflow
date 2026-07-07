use coflow_api::{
    CfdValue, CftContainer, CftSchemaTypeRef, DiagnosticSet, WriteCellRequest,
    WriteFieldPathSegment,
};
use coflow_cfd::ast::{CfdBlock, CfdBlockEntry, CfdRecord as AstRecord, CfdValue as AstValue};
use coflow_cfd::CfdAst;
use coflow_cft::Span;
use std::collections::BTreeMap;

use super::render::serialize_value_for_type;
use super::schema_nav::{
    dict_key_path_matches, type_after_dict_key_segment, type_after_field_segment,
    type_after_field_segment_for_ref, type_after_index_segment,
};
use super::diag;

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

enum WriteTarget {
    Replace { span: Span, ty: CftSchemaTypeRef },
    InsertTopLevel { ty: CftSchemaTypeRef },
    InsertNested {
        block_span: Span,
        depth: usize,
        field_name: String,
        ty: CftSchemaTypeRef,
    },
}

fn validate_value(v: &CfdValue) -> Result<(), DiagnosticSet> {
    match v {
        CfdValue::Ref(target_key) if target_key.is_empty() => Err(DiagnosticSet::one(diag(
            "CFD-WRITE",
            "cannot write empty reference; pick a target key first",
        ))),
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
    schema: &CftContainer,
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

fn full_value_span(value: &AstValue) -> Span {
    if let AstValue::Block(b) = value {
        if let Some((_, tm_span)) = &b.type_marker {
            return Span::new(tm_span.start, b.span.end);
        }
    }
    value.span()
}

fn locate_target(
    schema: &CftContainer,
    actual_type: &str,
    record: &AstRecord,
    path: &[WriteFieldPathSegment],
) -> Result<WriteTarget, DiagnosticSet> {
    let WriteFieldPathSegment::Field(name) = &path[0] else {
        return Err(DiagnosticSet::one(diag(
            "CFD-WRITE",
            "top-level path must start with a field name",
        )));
    };
    let Some(field) = find_field_in_record(record, name) else {
        if path.len() > 1 {
            return Err(DiagnosticSet::one(diag(
                "CFD-WRITE",
                format!("top-level field `{name}` not found in record"),
            )));
        }
        let Some(ty) = type_after_field_segment(schema, actual_type, name) else {
            return Err(DiagnosticSet::one(diag(
                "CFD-WRITE",
                format!("field `{name}` not found on type `{actual_type}`"),
            )));
        };
        return Ok(WriteTarget::InsertTopLevel { ty });
    };
    let Some(next_type) = type_after_field_segment(schema, actual_type, name) else {
        return Err(DiagnosticSet::one(diag(
            "CFD-WRITE",
            format!("field `{name}` not found on type `{actual_type}`"),
        )));
    };
    if path.len() == 1 {
        return Ok(WriteTarget::Replace {
            span: full_value_span(&field.value),
            ty: next_type,
        });
    }
    locate_target_in_value(schema, &next_type, &field.value, &path[1..], 1)
}

fn find_field_in_record<'a>(record: &'a AstRecord, name: &str) -> Option<&'a coflow_cfd::CfdField> {
    record.fields.iter().find(|f| f.name == name).or_else(|| {
        record.entries.iter().find_map(|e| match e {
            CfdBlockEntry::Field(f) if f.name == name => Some(f),
            _ => None,
        })
    })
}

fn locate_target_in_value(
    schema: &CftContainer,
    current_type: &CftSchemaTypeRef,
    value: &AstValue,
    path: &[WriteFieldPathSegment],
    depth: usize,
) -> Result<WriteTarget, DiagnosticSet> {
    if path.is_empty() {
        return Ok(WriteTarget::Replace {
            span: full_value_span(value),
            ty: current_type.clone(),
        });
    }
    match (&path[0], value) {
        (WriteFieldPathSegment::Field(name), AstValue::Block(block)) => {
            locate_field_target(schema, current_type, block, name, path, depth)
        }
        (WriteFieldPathSegment::Index(index), AstValue::Array(items, _)) => {
            locate_array_target(schema, current_type, items, *index, path, depth)
        }
        (WriteFieldPathSegment::DictKey(key), AstValue::Block(block)) => {
            locate_dict_target(schema, current_type, block, key, path, depth)
        }
        _ => Err(DiagnosticSet::one(diag(
            "CFD-WRITE",
            format!("cannot navigate path segment {:?} in value", path[0]),
        ))),
    }
}

#[allow(clippy::option_if_let_else)]
fn locate_field_target(
    schema: &CftContainer,
    current_type: &CftSchemaTypeRef,
    block: &CfdBlock,
    name: &str,
    path: &[WriteFieldPathSegment],
    depth: usize,
) -> Result<WriteTarget, DiagnosticSet> {
    let Some(next_type) = type_after_field_segment_for_ref(schema, current_type, name) else {
        return Err(DiagnosticSet::one(diag(
            "CFD-WRITE",
            format!("field `{name}` cannot be selected from this value"),
        )));
    };
    let field = block.entries.iter().find_map(|entry| match entry {
        CfdBlockEntry::Field(field) if field.name == name => Some(field),
        _ => None,
    });
    match field {
        Some(field) if path.len() == 1 => Ok(WriteTarget::Replace {
            span: full_value_span(&field.value),
            ty: next_type,
        }),
        Some(field) => {
            locate_target_in_value(schema, &next_type, &field.value, &path[1..], depth + 1)
        }
        None if path.len() == 1 => Ok(WriteTarget::InsertNested {
            block_span: block.span,
            depth,
            field_name: name.to_string(),
            ty: next_type,
        }),
        None => Err(DiagnosticSet::one(diag(
            "CFD-WRITE",
            format!(
                "field `{name}` is inherited from a `...spread` and the editor \
                 cannot drill further into it; edit the source record directly"
            ),
        ))),
    }
}

fn locate_array_target(
    schema: &CftContainer,
    current_type: &CftSchemaTypeRef,
    items: &[AstValue],
    index: usize,
    path: &[WriteFieldPathSegment],
    depth: usize,
) -> Result<WriteTarget, DiagnosticSet> {
    let Some(next_type) = type_after_index_segment(current_type) else {
        return Err(DiagnosticSet::one(diag(
            "CFD-WRITE",
            format!("array index `{index}` cannot be selected from this value"),
        )));
    };
    let item = items.get(index).ok_or_else(|| {
        DiagnosticSet::one(diag("CFD-WRITE", format!("index {index} out of bounds")))
    })?;
    if path.len() == 1 {
        Ok(WriteTarget::Replace {
            span: full_value_span(item),
            ty: next_type,
        })
    } else {
        locate_target_in_value(schema, &next_type, item, &path[1..], depth + 1)
    }
}

fn locate_dict_target(
    schema: &CftContainer,
    current_type: &CftSchemaTypeRef,
    block: &CfdBlock,
    key: &str,
    path: &[WriteFieldPathSegment],
    depth: usize,
) -> Result<WriteTarget, DiagnosticSet> {
    let Some((key_type, next_type)) = type_after_dict_key_segment(current_type) else {
        return Err(DiagnosticSet::one(diag(
            "CFD-WRITE",
            format!("dict key `{key}` cannot be selected from this value"),
        )));
    };
    let Some(field) = block.entries.iter().find_map(|entry| match entry {
        CfdBlockEntry::Field(field)
            if dict_key_path_matches(schema, &key_type, &field.name, key) =>
        {
            Some(field)
        }
        _ => None,
    }) else {
        return Err(DiagnosticSet::one(diag(
            "CFD-WRITE",
            format!("dict key `{key}` not found in source block"),
        )));
    };
    if path.len() == 1 {
        Ok(WriteTarget::Replace {
            span: full_value_span(&field.value),
            ty: next_type,
        })
    } else {
        locate_target_in_value(schema, &next_type, &field.value, &path[1..], depth + 1)
    }
}

pub(super) fn spread_entries_at_path<'a>(
    schema: &CftContainer,
    actual_type: &str,
    record: &'a AstRecord,
    path: &[WriteFieldPathSegment],
) -> Result<&'a [CfdBlockEntry], DiagnosticSet> {
    if path.is_empty() {
        return Ok(record.entries.as_slice());
    }
    let root_type = CftSchemaTypeRef::Named(actual_type.to_string());
    let Some((value, value_type)) =
        value_at_spread_path_segment(schema, record.entries.as_slice(), &root_type, &path[0])?
    else {
        return Err(DiagnosticSet::one(diag(
            "CFD-WRITE",
            "spread rewrite site was not found",
        )));
    };
    block_entries_at_path(schema, value, &value_type, &path[1..])
}

fn block_entries_at_path<'a>(
    schema: &CftContainer,
    value: &'a AstValue,
    ty: &CftSchemaTypeRef,
    path: &[WriteFieldPathSegment],
) -> Result<&'a [CfdBlockEntry], DiagnosticSet> {
    if path.is_empty() {
        let AstValue::Block(block) = value else {
            return Err(DiagnosticSet::one(diag(
                "CFD-WRITE",
                "spread rewrite site is not an object block",
            )));
        };
        return Ok(block.entries.as_slice());
    }
    match value {
        AstValue::Block(block) => {
            let Some((next, next_type)) =
                value_at_spread_path_segment(schema, block.entries.as_slice(), ty, &path[0])?
            else {
                return Err(DiagnosticSet::one(diag(
                    "CFD-WRITE",
                    "spread rewrite site was not found",
                )));
            };
            block_entries_at_path(schema, next, &next_type, &path[1..])
        }
        AstValue::Array(items, _) => {
            let WriteFieldPathSegment::Index(index) = path[0] else {
                return Err(DiagnosticSet::one(diag(
                    "CFD-WRITE",
                    format!("cannot navigate path segment {:?} in array value", path[0]),
                )));
            };
            let Some(item_type) = type_after_index_segment(ty) else {
                return Err(DiagnosticSet::one(diag(
                    "CFD-WRITE",
                    "array index cannot be selected from this value",
                )));
            };
            let Some(item) = items.get(index) else {
                return Err(DiagnosticSet::one(diag(
                    "CFD-WRITE",
                    format!("index {index} out of bounds while locating spread rewrite site"),
                )));
            };
            block_entries_at_path(schema, item, &item_type, &path[1..])
        }
        _ => Err(DiagnosticSet::one(diag(
            "CFD-WRITE",
            format!("cannot navigate path segment {:?} in value", path[0]),
        ))),
    }
}

fn value_at_spread_path_segment<'a>(
    schema: &CftContainer,
    entries: &'a [CfdBlockEntry],
    current_type: &CftSchemaTypeRef,
    segment: &WriteFieldPathSegment,
) -> Result<Option<(&'a AstValue, CftSchemaTypeRef)>, DiagnosticSet> {
    match segment {
        WriteFieldPathSegment::Field(field_name) => {
            let Some(next_type) =
                type_after_field_segment_for_ref(schema, current_type, field_name)
            else {
                return Err(DiagnosticSet::one(diag(
                    "CFD-WRITE",
                    format!("field `{field_name}` cannot be selected from this value"),
                )));
            };
            Ok(entries
                .iter()
                .find_map(|entry| match entry {
                    CfdBlockEntry::Field(field) if field.name == *field_name => Some(&field.value),
                    _ => None,
                })
                .map(|value| (value, next_type)))
        }
        WriteFieldPathSegment::DictKey(key) => {
            let Some((key_type, next_type)) = type_after_dict_key_segment(current_type) else {
                return Err(DiagnosticSet::one(diag(
                    "CFD-WRITE",
                    format!("dict key `{key}` cannot be selected from this value"),
                )));
            };
            Ok(entries
                .iter()
                .find_map(|entry| match entry {
                    CfdBlockEntry::Field(field)
                        if dict_key_path_matches(schema, &key_type, &field.name, key) =>
                    {
                        Some(&field.value)
                    }
                    _ => None,
                })
                .map(|value| (value, next_type)))
        }
        WriteFieldPathSegment::Index(index) => Err(DiagnosticSet::one(diag(
            "CFD-WRITE",
            format!("array index `{index}` cannot be selected from an object block"),
        ))),
    }
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
