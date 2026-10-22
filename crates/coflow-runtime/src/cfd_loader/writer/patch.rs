use crate::api::{DiagnosticSet, WriteCellRequest, WriteFieldPathSegment};
use crate::data_model::CfdValue;
use coflow_language::cfd::ast::CfdRecord as AstRecord;
use coflow_language::cfd::CfdAst;
use coflow_language::cft::CftSchema;
use coflow_language::source::Span;
use std::collections::BTreeMap;

use super::diag;
use super::render::serialize_value_for_type;
use super::schema_nav::type_after_field_segment;
use super::target::{locate_target, WriteTarget};
use super::CFD_INDENT;

// 补丁生成按路径形态完整分派，拆散会重复记录定位与类型校验上下文。
#[allow(clippy::too_many_lines)]
pub(super) fn apply_patch(
    source: &str,
    ast: &CfdAst,
    request: &WriteCellRequest<'_>,
) -> Result<String, DiagnosticSet> {
    let record = find_record(ast, request.actual_type, request.record_id).ok_or_else(|| {
        DiagnosticSet::one(diag(
            "CFD-WRITE",
            format!(
                "record `{}.{}` not found in AST",
                request.actual_type, request.record_id
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
    let root_path;
    // 有效模型保证路径合法；源码定位失败说明某个默认父值尚未实体化。
    // 此时写入修改后的完整顶层值，任意深度的缺省父对象都使用同一规则。
    let (target, new_value) = match locate_target(
        request.schema,
        request.actual_type,
        record,
        request.field_path,
    ) {
        Ok(target) => (target, request.new_value),
        Err(error) if request.field_path.len() > 1 => {
            let Some(materialized) = request.materialized_top_level else {
                return Err(error);
            };
            root_path = [WriteFieldPathSegment::Field(top_field.clone())];
            let target = locate_target(request.schema, request.actual_type, record, &root_path)?;
            (target, materialized)
        }
        Err(error) => return Err(error),
    };
    validate_value(new_value)?;

    match target {
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
            let depth = replacement_serialization_depth(source, span.start, new_value);
            let fragment =
                serialize_value_for_type(new_value, Some(request.schema), Some(&ty), depth);
            Ok(format!(
                "{}{}{}",
                &source[..span.start],
                fragment,
                &source[span.end..]
            ))
        }
        WriteTarget::InsertTopLevel {
            ty,
            previous_value_end,
        } => {
            let block_end = record.span.end.min(source.len());
            let insert_pos = find_closing_brace(source, block_end)?;
            let fragment = format!(
                "{CFD_INDENT}{top_field}: {},\n",
                serialize_value_for_type(new_value, Some(request.schema), Some(&ty), 2)
            );
            Ok(insert_field_fragment(
                source,
                insert_pos,
                previous_value_end,
                &fragment,
            ))
        }
        WriteTarget::InsertNested {
            previous_value_end,
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
                serialize_value_for_type(new_value, Some(request.schema), Some(&ty), depth + 2)
            );
            Ok(insert_field_fragment(
                source,
                insert_pos,
                previous_value_end,
                &fragment,
            ))
        }
    }
}

// 缺失末尾逗号的合法对象在追加字段时补齐分隔符，注释和未修改源码保持原样。
fn insert_field_fragment(
    source: &str,
    position: usize,
    previous: Option<usize>,
    fragment: &str,
) -> String {
    let mut prefix = source[..position].to_string();
    if let Some(end) = previous {
        let suffix = &source[end..position];
        let has_comma = coflow_language::lexical::tokenize_lossless(suffix)
            .iter()
            .any(|token| token.text(suffix) == ",");
        if !has_comma {
            prefix.insert(end, ',');
        }
    }
    if !prefix.ends_with('\n') {
        prefix.push('\n');
    }
    format!("{prefix}{fragment}{}", &source[position..])
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
        CfdValue::OptionSome(inner) => value_starts_object(inner),
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
        .find(|record| record.type_name == actual_type && record.id == key)
}

pub(super) fn apply_unset_field_patch(
    schema: &CftSchema,
    source: &str,
    ast: &CfdAst,
    actual_type: &str,
    record_id: &str,
    field_path: &[WriteFieldPathSegment],
) -> Result<String, DiagnosticSet> {
    if field_path.is_empty() {
        return Err(DiagnosticSet::one(diag(
            "CFD-WRITE",
            "field_path must not be empty",
        )));
    }
    let record = find_record(ast, actual_type, record_id).ok_or_else(|| {
        DiagnosticSet::one(diag(
            "CFD-WRITE",
            format!("record `{actual_type}.{record_id}` not found in AST"),
        ))
    })?;
    let WriteTarget::Replace { span, .. } = locate_target(schema, actual_type, record, field_path)?
    else {
        return Err(DiagnosticSet::one(diag(
            "CFD-WRITE",
            "field_path must identify an existing field to unset",
        )));
    };
    let field_span = field_span_for_value(record, span).ok_or_else(|| {
        DiagnosticSet::one(diag("CFD-WRITE", "unset must identify a named field"))
    })?;
    let removed = removed_field_span(source, field_span);
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

fn field_span_for_value(record: &AstRecord, value_span: Span) -> Option<Span> {
    use coflow_language::cfd::CfdValue as V;
    let mut fields = record.fields.iter().collect::<Vec<_>>();
    while let Some(field) = fields.pop() {
        if field.value.span() == value_span {
            return Some(field.span);
        }
        let mut values = vec![&field.value];
        while let Some(value) = values.pop() {
            match value {
                V::Block(block) => fields.extend(block.fields.iter()),
                V::Array(items, _) => values.extend(items.iter()),
                V::OptionSome(inner, _) => values.push(inner),
                _ => {}
            }
        }
    }
    None
}

fn removed_field_span(source: &str, field_span: Span) -> Span {
    // 使用字段 AST 跨度，单行记录不能把同一行的其他字段或记录头一起删除。
    let mut start = field_span.start.min(source.len());
    let line_start = source[..start].rfind('\n').map_or(0, |index| index + 1);
    let whole_line = source[line_start..start].trim().is_empty();
    if whole_line {
        start = line_start;
    }
    let mut end = field_span.end.min(source.len());
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
    } else if !whole_line {
        // 删除末尾字段时消除它前面的分隔逗号，保留右花括号及后面的源码。
        let prefix = source[..start].trim_end();
        if prefix.ends_with(',') {
            start = prefix.len() - 1;
        }
    }
    if whole_line && source[end..].starts_with('\n') {
        end += 1;
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

pub(super) fn validate_record_id(key: &str) -> Result<(), DiagnosticSet> {
    if key.trim().is_empty() {
        return Err(DiagnosticSet::one(diag(
            "CFD-WRITE",
            "record key must not be empty",
        )));
    }
    if let Some(reason) = coflow_language::lexical::record_id_ident_error(key) {
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
    let insert_at = record.id_span.end.saturating_sub(record.span.start);
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
