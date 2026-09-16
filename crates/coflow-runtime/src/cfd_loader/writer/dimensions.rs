use crate::api::{DiagnosticSet, DimensionWriteResult, WriteDimensionValueRequest};
use coflow_language::cfd::{parse_cfd, CfdValue as AstValue};
use coflow_language::source::Span;

use super::render::serialize_value_for_type;
use super::{diag, CfdWriter, CFD_INDENT};

impl CfdWriter {
    /// 维度变体与业务记录共址；写入只修改目标 `dimension` 块，避免重排同文件其他内容。
    pub(crate) fn write_dimension_value(
        &self,
        request: &WriteDimensionValueRequest<'_>,
    ) -> Result<DimensionWriteResult, DiagnosticSet> {
        let path = request.source.location.path();
        let source = self.read_source(path)?;
        let (ast, syntax) = parse_cfd(&source);
        if !syntax.is_empty() {
            return Err(DiagnosticSet::one(diag(
                "CFD-DIMENSION-WRITE",
                syntax
                    .into_iter()
                    .map(|item| item.message)
                    .collect::<Vec<_>>()
                    .join("; "),
            )));
        }
        let record = ast
            .records
            .iter()
            .find(|record| {
                record.key == request.source_key.as_str()
                    && request
                        .schema
                        .schema
                        .resolve_record_type_name(&record.type_name)
                        .is_ok_and(|name| name == request.actual_type)
            })
            .ok_or_else(|| {
                DiagnosticSet::one(diag(
                    "CFD-DIMENSION-WRITE",
                    format!(
                        "record `{}:{}` not found",
                        request.source_key, request.actual_type
                    ),
                ))
            })?;
        let field = record
            .fields
            .iter()
            .find(|field| field.name == request.schema.source_field.name.as_str())
            .ok_or_else(|| {
                DiagnosticSet::one(diag(
                    "CFD-DIMENSION-WRITE",
                    format!(
                        "dimension field `{}` is not materialized",
                        request.schema.source_field.name
                    ),
                ))
            })?;
        let AstValue::Dimension(dimension) = &field.value else {
            return Err(DiagnosticSet::one(diag(
                "CFD-DIMENSION-WRITE",
                format!(
                    "field `{}` is not a dimension block",
                    request.schema.source_field.name
                ),
            )));
        };
        if request.variant.as_str() == "default" {
            return Err(DiagnosticSet::one(diag(
                "CFD-DIMENSION-WRITE",
                "the default value must be edited through the regular field API",
            )));
        }

        let existing = dimension
            .fields
            .iter()
            .find(|field| field.name == request.variant.as_str());
        let rewritten = match (existing, request.new_value) {
            (Some(field), Some(value)) => replace_span(
                &source,
                field.value.span(),
                &serialize_value_for_type(
                    value,
                    Some(request.schema.schema),
                    Some(&request.schema.source_field.value_type),
                    3,
                ),
            )?,
            (Some(field), None) => remove_field(&source, field.span)?,
            (None, None) => source.to_string(),
            (None, Some(value)) => {
                let close = find_closing_brace(&source, dimension.span)?;
                let fragment = format!(
                    "{CFD_INDENT}{CFD_INDENT}{}: {},\n{CFD_INDENT}",
                    request.variant,
                    serialize_value_for_type(
                        value,
                        Some(request.schema.schema),
                        Some(&request.schema.source_field.value_type),
                        3,
                    ),
                );
                format!("{}{}{}", &source[..close], fragment, &source[close..])
            }
        };
        let changed = rewritten != source;
        if changed {
            self.write_source(path, &rewritten)?;
        }
        Ok(DimensionWriteResult { changed })
    }
}

fn replace_span(source: &str, span: Span, replacement: &str) -> Result<String, DiagnosticSet> {
    if span.start > span.end || span.end > source.len() {
        return Err(DiagnosticSet::one(diag(
            "CFD-DIMENSION-WRITE",
            "dimension value span is invalid",
        )));
    }
    Ok(format!(
        "{}{}{}",
        &source[..span.start],
        replacement,
        &source[span.end..]
    ))
}

fn remove_field(source: &str, span: Span) -> Result<String, DiagnosticSet> {
    if span.start > span.end || span.end > source.len() {
        return Err(DiagnosticSet::one(diag(
            "CFD-DIMENSION-WRITE",
            "dimension field span is invalid",
        )));
    }
    let mut start = span.start;
    while start > 0 && !source[..start].ends_with('\n') {
        start -= 1;
    }
    let mut end = span.end;
    while end < source.len() && matches!(source.as_bytes()[end], b' ' | b'\t') {
        end += 1;
    }
    if source.as_bytes().get(end) == Some(&b',') {
        end += 1;
    }
    while end < source.len() && matches!(source.as_bytes()[end], b' ' | b'\t') {
        end += 1;
    }
    if source.as_bytes().get(end) == Some(&b'\r') {
        end += 1;
    }
    if source.as_bytes().get(end) == Some(&b'\n') {
        end += 1;
    }
    Ok(format!("{}{}", &source[..start], &source[end..]))
}

fn find_closing_brace(source: &str, span: Span) -> Result<usize, DiagnosticSet> {
    let end = span.end.min(source.len());
    source[..end].rfind('}').ok_or_else(|| {
        DiagnosticSet::one(diag(
            "CFD-DIMENSION-WRITE",
            "dimension block has no closing brace",
        ))
    })
}
