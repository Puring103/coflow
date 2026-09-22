//! LSP feature providers for `.cfd` data files.
//!
//! Each function takes the parsed [`CfdAst`] (plus optional compiled schema)
//! and returns typed results shared by the editor and LSP adapter.

// `${1:name}` 是 LSP snippet 占位符，不是 Rust 格式化参数。
#![allow(clippy::literal_string_with_formatting_args)]

mod builder_completion;
mod definition;
use crate::service::*;
pub use definition::{definition_field_name, definition_ref_target, definition_type_name};

use coflow_core::schema::{CftSchema, CftValueType};
use coflow_language::cfd::{
    parse_cfd, tokenize_cfd, CfdAst, CfdBitExpr, CfdBitExprKind, CfdFunction, CfdRecord,
    CfdSyntaxDiagnostic, CfdValue, CFD_FUNCTION_BUILTINS, CFD_FUNCTION_KEYWORDS,
    CFD_FUNCTION_TYPES,
};
use coflow_language::lexical::{is_identifier_continue, LosslessTokenKind};
use coflow_language::source::Span;

use super::semantic_tokens::{
    MOD_DECLARATION, MOD_RECORD, MOD_REFERENCE, MOD_SCHEMA, SEM_COMMENT, SEM_ENUM_MEMBER,
    SEM_FUNCTION, SEM_KEYWORD, SEM_NUMBER, SEM_OPERATOR, SEM_PARAMETER, SEM_PROPERTY,
    SEM_RECORD_KEY, SEM_STRING, SEM_TYPE, SEM_VARIABLE,
};
use super::LspBuild;

// ── Public helpers used by LspServer ─────────────────────────────────────────

/// Build LSP diagnostics from CFD syntax errors.
pub fn syntax_diagnostics(source: &str, errors: &[CfdSyntaxDiagnostic]) -> Vec<LanguageDiagnostic> {
    errors
        .iter()
        .map(|e| {
            let range = byte_range(source, e.span.start, e.span.end.max(e.span.start + 1));
            LanguageDiagnostic {
                range: (range).clone(),
                severity: 1,
                source: Some(("coflow-language").to_string()),
                message: (e.message).to_string(),
                ..Default::default()
            }
        })
        .collect()
}

/// Document symbols: one entry per top-level CFD record.
pub fn document_symbols(source: &str, ast: &CfdAst) -> Vec<DocumentSymbol> {
    let symbols: Vec<DocumentSymbol> = ast
        .records
        .iter()
        .map(|record| {
            let name_range = byte_range(source, record.key_span.start, record.key_span.end);
            let full_range = byte_range(source, record.span.start, record.span.end);
            DocumentSymbol {
                name: (record.key).to_string(),
                detail: Some((record.type_name).to_string()),
                kind: 5,
                range: (full_range).clone(),
                selection_range: (name_range).clone(),
                children: (field_symbols(source, record)).to_vec(),
                ..Default::default()
            }
        })
        .collect();
    symbols
}

fn field_symbols(source: &str, record: &CfdRecord) -> Vec<DocumentSymbol> {
    record
        .fields()
        .map(|field| {
            let name_range = byte_range(source, field.name_span.start, field.name_span.end);
            let full_range = byte_range(source, field.span.start, field.span.end);
            DocumentSymbol {
                name: (field.name).to_string(),
                kind: 8,
                range: (full_range).clone(),
                selection_range: (name_range).clone(),
                children: (Vec::new()).to_vec(),
                ..Default::default()
            }
        })
        .collect()
}

/// 悬浮信息：根据字段或类型位置查询 schema。
pub fn hover(
    source: &str,
    ast: &CfdAst,
    schema: Option<&CftSchema>,
    offset: usize,
) -> Option<Hover> {
    for record in &ast.records {
        if span_contains(record.type_span, offset) {
            let detail = schema
                .and_then(|s| s.resolve_type(&record.type_name))
                .map_or_else(
                    || format!("`{}`", record.type_name),
                    |t| {
                        let mut md = format!("```\ntype {}", t.name);
                        if t.is_abstract {
                            md.push_str(" (abstract)");
                        }
                        if t.is_sealed {
                            md.push_str(" (sealed)");
                        }
                        md.push_str("\n```");
                        md
                    },
                );
            return Some(Hover {
                contents: Markup {
                    kind: ("markdown").to_string(),
                    value: (detail).to_string(),
                    ..Default::default()
                },
                range: (byte_range(source, record.type_span.start, record.type_span.end)).clone(),
                ..Default::default()
            });
        }
        for field in record.fields() {
            if span_contains(field.name_span, offset) {
                let detail = schema
                    .and_then(|s| s.resolve_type(&record.type_name))
                    .and_then(|t| t.all_fields().find(|f| f.name.as_str() == field.name))
                    .map_or_else(
                        || format!("`{}`", field.name),
                        |f| format!("```\n{}: {}\n```", f.name, fmt_value_type(&f.value_type)),
                    );
                return Some(Hover {
                    contents: Markup {
                        kind: ("markdown").to_string(),
                        value: (detail).to_string(),
                        ..Default::default()
                    },
                    range: (byte_range(source, field.name_span.start, field.name_span.end)).clone(),
                    ..Default::default()
                });
            }
        }
    }
    None
}

/// 查找包含当前偏移的函数值。
fn function_at(ast: &CfdAst, offset: usize) -> Option<&CfdFunction> {
    ast.records
        .iter()
        .flat_map(|record| record.fields.iter())
        .find_map(|field| function_in_value(&field.value, offset))
}

fn function_in_value(value: &CfdValue, offset: usize) -> Option<&CfdFunction> {
    match value {
        CfdValue::Function(function) if span_contains(function.span, offset) => Some(function),
        CfdValue::Block(block) => block
            .fields
            .iter()
            .find_map(|field| function_in_value(&field.value, offset)),
        CfdValue::Array(values, _) => values
            .iter()
            .find_map(|value| function_in_value(value, offset)),
        CfdValue::Dimension(dimension) => dimension
            .fields
            .iter()
            .find_map(|field| function_in_value(&field.value, offset)),
        _ => None,
    }
}

fn span_contains(span: Span, offset: usize) -> bool {
    offset >= span.start && offset < span.end.max(span.start + 1)
}

fn fmt_value_type(ty: &CftValueType) -> String {
    ty.display_label()
}

pub fn byte_range(source: &str, start: usize, end: usize) -> LanguageRange {
    super::position::byte_range(source, start, end)
}

/// 与 [`byte_range`] 输出一致的 range，但复用预建行索引。
///
/// 逐条记录调用 [`byte_range`] 会从文件头重新扫描，在整项目索引时退化为二次
/// 复杂度；行索引只构建一次。
pub fn byte_range_with_index(
    index: &coflow_project::LineIndex,
    source: &str,
    start: usize,
    end: usize,
) -> LanguageRange {
    super::position::byte_range_indexed(index, source, start, end)
}

mod functions;
use functions::function_scoped_completions;
pub(crate) use functions::{function_document, function_source_completion_items_at};

mod tokens;
use tokens::{collect_function_tokens, TokenCollector};
pub(crate) use tokens::{semantic_tokens, visit_function_semantic_tokens};

mod completion;
use completion::collect_bit_expr_values;
#[cfg(test)]
pub(crate) use completion::completion;
pub(crate) use completion::completion_with_build;

mod recovery;
use recovery::{
    completion_context, incomplete_group_key_at, incomplete_group_keys, record_context_at,
    recovered_field_names, CompletionContext,
};
