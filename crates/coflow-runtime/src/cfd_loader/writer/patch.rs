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
    let patch = field_patch(source, record, request)?;
    Ok(patch.apply(source))
}

/// 补丁只拥有变化片段；未变化源码由批次收尾时一次复制。
pub(super) struct FieldPatch {
    pub(super) span: Span,
    pub(super) replacement: String,
    pub(super) formatting_start: Option<usize>,
    pub(super) insertion: Option<FieldInsertion>,
    pub(super) independent_syntax: bool,
}

pub(super) struct FieldInsertion {
    pub(super) field: String,
    pub(super) fragment_start: usize,
}

impl FieldPatch {
    pub(super) fn apply(&self, source: &str) -> String {
        let mut result = String::with_capacity(
            source.len() - (self.span.end - self.span.start) + self.replacement.len(),
        );
        result.push_str(&source[..self.span.start]);
        result.push_str(&self.replacement);
        result.push_str(&source[self.span.end..]);
        result
    }
}

// 所有字段形态共用补丁表示，包含集合、对象、嵌套路径和缺省父对象实体化。
#[allow(clippy::too_many_lines)]
pub(super) fn field_patch(
    source: &str,
    record: &AstRecord,
    request: &WriteCellRequest<'_>,
) -> Result<FieldPatch, DiagnosticSet> {
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
    match target {
        WriteTarget::Replace { span, ty, .. } => {
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
            let independent_syntax = has_independent_syntax(new_value, &fragment);
            Ok(FieldPatch {
                span,
                replacement: fragment,
                formatting_start: Some(
                    source[..span.start]
                        .rfind('\n')
                        .map_or(0, |index| index + 1),
                ),
                insertion: None,
                independent_syntax,
            })
        }
        WriteTarget::InsertTopLevel {
            ty,
            previous_value_end,
        } => {
            let block_end = record.span.end.min(source.len());
            let insert_pos = find_closing_brace(source, block_end)?;
            let value = serialize_value_for_type(new_value, Some(request.schema), Some(&ty), 2);
            let fragment = format!("{CFD_INDENT}{top_field}: {value},\n");
            let mut patch =
                insert_field_patch(source, insert_pos, previous_value_end, &fragment, top_field);
            patch.independent_syntax = has_independent_syntax(new_value, &value);
            Ok(patch)
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
            let value =
                serialize_value_for_type(new_value, Some(request.schema), Some(&ty), depth + 2);
            let fragment = format!("{indent}{field_name}: {value},\n{outer}");
            let mut patch = insert_field_patch(
                source,
                insert_pos,
                previous_value_end,
                &fragment,
                &field_name,
            );
            patch.independent_syntax = has_independent_syntax(new_value, &value);
            Ok(patch)
        }
    }
}

/// 独立补丁的前后子树都已经过真实解析；以语法节点计数保留每一步的文件预算。
pub(super) fn field_node_change(
    before: &AstRecord,
    after: &AstRecord,
    request: &WriteCellRequest<'_>,
) -> Result<(u64, u64), DiagnosticSet> {
    let root_path;
    let (old_target, path) = match locate_target(
        request.schema,
        request.actual_type,
        before,
        request.field_path,
    ) {
        Ok(target) => (target, request.field_path),
        Err(error) => {
            if request.field_path.len() <= 1 || request.materialized_top_level.is_none() {
                return Err(error);
            }
            // 与 field_patch 的默认父对象实体化一致，实际补丁可能替换整个顶层值。
            root_path = [request.field_path[0].clone()];
            let target = locate_target(request.schema, request.actual_type, before, &root_path)?;
            (target, root_path.as_slice())
        }
    };
    let (removed, inserted_field) = match old_target {
        WriteTarget::Replace { value, .. } => (value.structural_node_count(), 0),
        WriteTarget::InsertTopLevel { .. } | WriteTarget::InsertNested { .. } => (0, 1),
    };
    let new_target = locate_target(request.schema, request.actual_type, after, path)?;
    match new_target {
        WriteTarget::Replace { value, .. } => {
            Ok((removed, value.structural_node_count() + inserted_field))
        }
        WriteTarget::InsertTopLevel { .. } | WriteTarget::InsertNested { .. } => {
            Err(DiagnosticSet::one(diag(
                "CFD-WRITE",
                "written field is absent from the parsed batch candidate",
            )))
        }
    }
}

// 缺失末尾逗号的合法对象在追加字段时补齐分隔符，注释和未修改源码保持原样。
fn insert_field_patch(
    source: &str,
    position: usize,
    previous: Option<usize>,
    fragment: &str,
    field: &str,
) -> FieldPatch {
    let start = previous.unwrap_or(position);
    let mut replacement = String::new();
    if let Some(end) = previous {
        let suffix = &source[end..position];
        let has_comma = coflow_language::lexical::tokenize_lossless(suffix)
            .iter()
            .any(|token| token.text(suffix) == ",");
        if !has_comma {
            replacement.push(',');
        }
        replacement.push_str(suffix);
    }
    let ends_with_newline = if replacement.is_empty() {
        source[..position].ends_with('\n')
    } else {
        replacement.ends_with('\n')
    };
    if !ends_with_newline {
        replacement.push('\n');
    }
    let fragment_start = replacement.len();
    replacement.push_str(fragment);
    FieldPatch {
        span: Span::new(start, position),
        replacement,
        formatting_start: None,
        insertion: Some(FieldInsertion {
            field: field.to_string(),
            fragment_start,
        }),
        independent_syntax: true,
    }
}

fn has_independent_syntax(value: &CfdValue, rendered: &str) -> bool {
    fn contains_source(value: &CfdValue) -> bool {
        match value {
            CfdValue::FormattedString(_) => true,
            CfdValue::Object(object) => object.fields.values().any(contains_source),
            CfdValue::Array(items) => items.iter().any(contains_source),
            CfdValue::Dict(items) => items.iter().any(|(_, value)| contains_source(value)),
            CfdValue::OptionSome(value) => contains_source(value),
            _ => false,
        }
    }
    if !contains_source(value) {
        return true;
    }
    // 模板携带原始源码。只有封闭的单个值才能合批，防止后续补丁意外修复
    // 本应在当前操作报告的未闭合字符串、注释或额外字段。
    const PREFIX: &str = "patch: Patch { value: ";
    let wrapped = format!("{PREFIX}{rendered} }}");
    let (ast, diagnostics) = coflow_language::cfd::parse_cfd(&wrapped);
    diagnostics.is_empty()
        && ast.records.len() == 1
        && ast.records[0].fields.len() == 1
        && ast.records[0].fields[0].value.span().end == PREFIX.len() + rendered.len()
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
    // 借用补丁并按源码顺序一次拼接，重排大量记录时不反复移动整段文件尾部。
    let mut sorted = replacements.iter().collect::<Vec<_>>();
    sorted.sort_by_key(|(span, _)| span.start);
    for (span, _) in &sorted {
        if source.get(span.start..span.end).is_none() {
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
    let mut cursor = 0;
    let mut capacity = source.len();
    for (span, replacement) in &sorted {
        if span.start < cursor {
            return Err(DiagnosticSet::one(diag(
                "CFD-WRITE",
                "source replacement spans overlap",
            )));
        }
        cursor = span.end;
        capacity = capacity - (span.end - span.start) + replacement.len();
    }
    let mut out = String::with_capacity(capacity);
    cursor = 0;
    for (span, replacement) in sorted {
        out.push_str(&source[cursor..span.start]);
        out.push_str(replacement);
        cursor = span.end;
    }
    out.push_str(&source[cursor..]);
    Ok(out)
}

#[cfg(test)]
mod replacement_tests {
    #![allow(clippy::expect_used)]

    use super::*;

    #[test]
    fn source_replacements_keep_order_unicode_and_duplicate_span_semantics() {
        let source = "甲 first, second 乙";
        let first = source.find("first").expect("first");
        let second = source.find("second").expect("second");
        let edits = [
            (Span::new(second, second + 6), "二".into()),
            (Span::new(first, first + 5), "一".into()),
            (Span::new(first, first + 5), "ignored duplicate".into()),
            (Span::new(source.len(), source.len()), "\n".into()),
        ];
        assert_eq!(
            replace_spans(source, &edits).expect("replacements"),
            "甲 一, 二 乙\n"
        );
        assert_eq!(
            replace_spans(source, &[]).expect("empty replacement"),
            source
        );
    }

    #[test]
    fn invalid_or_overlapping_source_spans_return_diagnostics() {
        assert!(replace_spans("甲abc", &[(Span::new(1, 2), "x".into())]).is_err());
        assert!(replace_spans("abc", &[(Span::new(2, 4), "x".into())]).is_err());
        assert!(replace_spans(
            "abc",
            &[(Span::new(0, 2), "x".into()), (Span::new(1, 3), "y".into()),]
        )
        .is_err());
    }
}
