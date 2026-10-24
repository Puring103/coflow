use crate::api::{
    DiagnosticSet, DimensionSourceResult, RewriteDimensionRecordRequest, WriteDimensionValueRequest,
};
use coflow_language::cfd::parse_cfd;

use super::CfdWriter;

impl CfdWriter {
    #[expect(
        clippy::too_many_lines,
        reason = "按语言或命令分支集中处理，保持分派与各分支语义可直接核对。"
    )]
    pub(crate) fn write_dimension_value(
        &self,
        request: &WriteDimensionValueRequest<'_>,
    ) -> Result<DimensionSourceResult, DiagnosticSet> {
        let path = request.source.location.path();
        let text = match self.read_source(path) {
            Ok(text) => text,
            Err(_) if !path.exists() => String::new(),
            Err(error) => return Err(error),
        };
        let (ast, diagnostics) = parse_cfd(&text);
        super::ensure_parse_ok(path, &text, &diagnostics)?;
        let dimension = request.schema.dimension.name.as_str();
        let variant = request.variant.as_str();
        let field_name = request.schema.source_field.name.as_str();
        let matches_record = |record: &&coflow_language::cfd::CfdRecord| {
            record.id == request.source_id.as_str()
                && request.schema.schema.inheritance_root(&record.type_name)
                    == request
                        .schema
                        .schema
                        .inheritance_root(request.schema.source_type.name.as_str())
        };
        let blocks = ast
            .dimensions
            .iter()
            .filter(|block| block.dimension == dimension && block.variant == variant)
            .collect::<Vec<_>>();
        let records = blocks
            .iter()
            .flat_map(|block| &block.records)
            .filter(matches_record)
            .collect::<Vec<_>>();
        let existing = records
            .iter()
            .find_map(|record| record.fields.iter().find(|field| field.name == field_name));
        let mut patches = Vec::new();
        if let Some(field) = existing {
            if let Some(value) = request.new_value {
                let span = match &field.value {
                    coflow_language::cfd::CfdValue::Block(block) => block
                        .type_marker
                        .as_ref()
                        .map_or(block.span, |(_, marker)| {
                            coflow_language::source::Span::new(marker.start, block.span.end)
                        }),
                    value => value.span(),
                };
                patches.push((
                    span,
                    super::render::serialize_value_for_type(
                        value,
                        Some(request.schema.schema),
                        Some(&request.schema.source_field.value_type),
                        2,
                    ),
                ));
            } else {
                // 仅删除覆盖字段与紧随的分隔逗号，保留同一行的其他字段及注释。
                let mut span = field.span;
                let tail = &text[span.end..];
                let token = coflow_language::lexical::tokenize_lossless(tail)
                    .into_iter()
                    .find(|token| !token.is_trivia());
                if let Some(token) = token.filter(|token| token.text(tail) == ",") {
                    span.end += token.span.end;
                }
                patches.push((span, String::new()));
            }
        } else if let Some(value) = request.new_value {
            let value = super::render::serialize_value_for_type(
                value,
                Some(request.schema.schema),
                Some(&request.schema.source_field.value_type),
                2,
            );
            if let Some(record) = records.first() {
                if let Some(last) = record.fields.last() {
                    let tail = &text[last.span.end..record.span.end - 1];
                    let separated = coflow_language::lexical::tokenize_lossless(tail)
                        .into_iter()
                        .find(|token| !token.is_trivia())
                        .is_some_and(|token| token.text(tail) == ",");
                    if !separated {
                        patches.push((
                            coflow_language::source::Span::new(last.span.end, last.span.end),
                            ",".into(),
                        ));
                    }
                }
                let end = record.span.end - 1;
                patches.push((
                    coflow_language::source::Span::new(end, end),
                    format!("\n    {field_name}: {value},\n  "),
                ));
            } else {
                let record = format!(
                    "  {}: {} {{\n    {field_name}: {value},\n  }}\n",
                    request.source_id, request.schema.source_type.name
                );
                if let Some(block) = blocks.first() {
                    let end = block.span.end - 1;
                    patches.push((
                        coflow_language::source::Span::new(end, end),
                        format!("\n{record}"),
                    ));
                } else {
                    let end = text.len();
                    patches.push((
                        coflow_language::source::Span::new(end, end),
                        format!("\n{dimension}[{variant}] {{\n{record}}}\n"),
                    ));
                }
            }
        }
        let result = super::patch::replace_spans(&text, &patches)?;
        if result == text {
            return Ok(DimensionSourceResult { changed: false });
        }
        self.write_source(path, &result)?;
        Ok(DimensionSourceResult { changed: true })
    }

    pub(crate) fn rewrite_dimension_record(
        &self,
        request: &RewriteDimensionRecordRequest<'_>,
    ) -> Result<DimensionSourceResult, DiagnosticSet> {
        let path = request.source.location.path();
        let (text, ast) = self.read_or_parse(path)?;
        let root = request.schema.inheritance_root(request.actual_type);
        let mut patches = Vec::new();
        // 同一主体的覆盖可跨维度、变体与文件，按记录身份更新全部覆盖块。
        for record in ast.dimensions.iter().flat_map(|block| &block.records) {
            if record.id != request.old_id.as_str()
                || request.schema.inheritance_root(&record.type_name) != root
            {
                continue;
            }
            if let Some(new_id) = request.new_id {
                patches.push((record.id_span, new_id.to_string()));
            } else {
                patches.push((record.span, String::new()));
            }
        }
        let result = super::patch::replace_spans(&text, &patches)?;
        if result == text {
            return Ok(DimensionSourceResult { changed: false });
        }
        self.write_source(path, &result)?;
        Ok(DimensionSourceResult { changed: true })
    }
}
