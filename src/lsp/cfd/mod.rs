//! LSP feature providers for `.cfd` data files.
//!
//! Each function takes the parsed [`CfdAst`] (plus optional compiled schema)
//! and returns a JSON [`Value`] ready to send as an LSP response.

use coflow_language::cfd::{
    parse_cfd, CfdAst, CfdBitExpr, CfdBitExprKind, CfdField, CfdFormatSegment, CfdFunction,
    CfdRecord, CfdSyntaxDiagnostic, CfdValue,
};
use coflow_language::{CftSchema, CftValueType, Span};
use serde_json::{json, Value};

use super::semantic_tokens::{
    MOD_DECLARATION, MOD_RECORD, MOD_REFERENCE, MOD_SCHEMA, SEM_COMMENT, SEM_ENUM_MEMBER,
    SEM_FUNCTION, SEM_KEYWORD, SEM_NAMESPACE, SEM_NUMBER, SEM_OPERATOR, SEM_PARAMETER,
    SEM_PROPERTY, SEM_STRING, SEM_TYPE, SEM_VARIABLE,
};
use super::LspBuild;

const FUNCTION_KEYWORDS: &[&str] = &[
    "fn", "var", "return", "if", "else", "match", "for", "while", "break", "continue",
    "in", "is", "true", "false", "None", "Some", "Ok", "Err",
];
const FUNCTION_TYPES: &[&str] = &["int", "float", "bool", "string", "Option", "Result"];
const FUNCTION_BUILTINS: &[&str] = &[
    "len", "map", "filter", "fold", "find", "any", "all", "contains", "starts_with",
    "ends_with",
];

// ── Public helpers used by LspServer ─────────────────────────────────────────

/// Build LSP diagnostics from CFD syntax errors.
pub fn syntax_diagnostics(source: &str, errors: &[CfdSyntaxDiagnostic]) -> Vec<Value> {
    errors
        .iter()
        .map(|e| {
            let range = byte_range(source, e.span.start, e.span.end.max(e.span.start + 1));
            json!({
                "range": range,
                "severity": 1,
                "source": "coflow-language",
                "message": e.message,
            })
        })
        .collect()
}

/// Document symbols: one entry per top-level CFD record.
pub fn document_symbols(source: &str, ast: &CfdAst) -> Value {
    let symbols: Vec<Value> = ast
        .records
        .iter()
        .map(|record| {
            let name_range = byte_range(source, record.key_span.start, record.key_span.end);
            let full_range = byte_range(source, record.span.start, record.span.end);
            json!({
                "name": record.key,
                "detail": record.type_name,
                "kind": 5,  // Class
                "range": full_range,
                "selectionRange": name_range,
                "children": field_symbols(source, record),
            })
        })
        .collect();
    json!(symbols)
}

fn field_symbols(source: &str, record: &CfdRecord) -> Vec<Value> {
    record
        .fields()
        .map(|field| {
            let name_range = byte_range(source, field.name_span.start, field.name_span.end);
            let full_range = byte_range(source, field.span.start, field.span.end);
            json!({
                "name": field.name,
                "kind": 8,  // Field
                "range": full_range,
                "selectionRange": name_range,
                "children": [],
            })
        })
        .collect()
}

/// Semantic tokens for a CFD file (delta encoding as per LSP spec).
pub fn semantic_tokens(source: &str, ast: &CfdAst, schema: Option<&CftSchema>) -> Value {
    let mut collector = TokenCollector::new(source);

    // Lex all comment spans.
    collect_comment_tokens(source, &mut collector);

    if let Some(namespace) = &ast.namespace {
        collector.add_plain(
            Span::new(namespace.span.start, namespace.span.start + "namespace".len()),
            SEM_KEYWORD,
        );
        collector.add(namespace.path_span, SEM_NAMESPACE, MOD_DECLARATION | MOD_SCHEMA);
    }
    for import in &ast.uses {
        collector.add_plain(
            Span::new(import.span.start, import.span.start + "use".len()),
            SEM_KEYWORD,
        );
        collector.add(import.path_span, SEM_TYPE, MOD_REFERENCE | MOD_SCHEMA);
        if let Some((_, alias_span)) = &import.alias {
            collector.add(*alias_span, SEM_TYPE, MOD_DECLARATION | MOD_SCHEMA);
        }
    }

    // Walk the AST for structured tokens.
    for record in &ast.records {
        collector.add(record.key_span, SEM_NAMESPACE, MOD_DECLARATION | MOD_RECORD);
        collector.add(record.type_span, SEM_TYPE, MOD_REFERENCE | MOD_SCHEMA);
        for field in &record.fields {
            collector.add(field.name_span, SEM_PROPERTY, MOD_DECLARATION | MOD_SCHEMA);
            collect_value_tokens(&field.value, &mut collector);
        }
    }
    if let Some(schema) = schema {
        for (_, span, _) in incomplete_group_keys(source, schema) {
            collector.add(span, SEM_NAMESPACE, MOD_DECLARATION | MOD_RECORD);
        }
    }

    collector.into_lsp_data()
}

/// Handles the editor's function-body virtual document request.
pub fn function_document(params: &Value) -> Value {
    let Some(original) = params.get("source").and_then(Value::as_str) else {
        return Value::Null;
    };
    let Some(parts) = function_parts(original) else {
        return json!({
            "source": original,
            "signature": "fn",
            "body": original,
            "bodyRange": byte_range(original, 0, original.len()),
            "diagnostics": [{
                "range": byte_range(original, 0, original.len().min(1)),
                "severity": 1,
                "source": "coflow-language",
                "message": "invalid function source",
            }],
            "semanticTokens": { "data": [] },
            "completions": [],
        });
    };
    let requested_body = params
        .get("body")
        .and_then(Value::as_str)
        .unwrap_or_else(|| parts.body.trim());
    let body = if parts.multiline {
        requested_body.trim_matches('\n')
    } else {
        requested_body.trim()
    };
    let replacement = if parts.multiline {
        format!("\n{body}\n")
    } else {
        format!(" {body} ")
    };
    let source = format!("{}{}{}", parts.prefix, replacement, parts.suffix);
    let body_start = parts.prefix.len() + 1;
    let body_end = body_start + body.len();
    let mut collector = TokenCollector::new(&source);
    collect_function_tokens(Span::new(0, source.len()), &source, &mut collector);
    let diagnostics = function_body_diagnostics(&source, body_start, body);
    json!({
        "source": source,
        "signature": parts.signature,
        "body": body,
        "bodyRange": byte_range(&source, body_start, body_end),
        "diagnostics": diagnostics,
        "semanticTokens": collector.into_lsp_data(),
        "completions": function_completion_items_with_locals(parts.signature, &source),
    })
}

struct FunctionParts<'a> {
    prefix: &'a str,
    body: &'a str,
    suffix: &'a str,
    signature: &'a str,
    multiline: bool,
}

fn function_parts(source: &str) -> Option<FunctionParts<'_>> {
    const PREFIX: &str = "__function: __EditorFunction { value: ";
    const SUFFIX: &str = "\n}";
    let document = format!("{PREFIX}{source}{SUFFIX}");
    let (ast, diagnostics) = parse_cfd(&document);
    if !diagnostics.is_empty() {
        return None;
    }
    let function = ast
        .records
        .first()?
        .fields
        .first()
        .and_then(|field| match &field.value {
            CfdValue::Function(function) => Some(function),
            _ => None,
        })?;
    let start = function.body_span.start.checked_sub(PREFIX.len())?;
    let end = function.body_span.end.checked_sub(PREFIX.len())?;
    let open = start.checked_sub(1)?;
    Some(FunctionParts {
        prefix: source.get(..start)?,
        body: source.get(start..end)?,
        suffix: source.get(end..)?,
        signature: source.get(..open)?.trim(),
        multiline: source.get(start..end)?.contains('\n') || source.contains('\n'),
    })
}

fn function_body_diagnostics(
    function_source: &str,
    body_start: usize,
    body: &str,
) -> Vec<Value> {
    const PREFIX: &str = "__function: __EditorFunction { value: ";
    let document = format!("{PREFIX}{function_source}\n}}");
    let (_, diagnostics) = parse_cfd(&document);
    let body_document_start = PREFIX.len() + body_start;
    diagnostics
        .into_iter()
        .map(|diagnostic| {
            let relative = diagnostic.span.start.saturating_sub(body_document_start);
            let start = relative.min(body.len().saturating_sub(1));
            let end = (start + diagnostic.span.end.saturating_sub(diagnostic.span.start).max(1))
                .min(body.len());
            json!({
                "range": byte_range(body, start, end),
                "severity": 1,
                "source": "coflow-language",
                "message": diagnostic.message,
            })
        })
        .collect()
}

fn function_completion_items(signature: &str) -> Vec<Value> {
    let mut items = FUNCTION_KEYWORDS
        .iter()
        .map(|label| json!({ "label": label, "kind": 14 }))
        .chain(FUNCTION_TYPES.iter().map(|label| json!({ "label": label, "kind": 7 })))
        .collect::<Vec<_>>();
    items.extend(FUNCTION_BUILTINS.iter().map(|label| {
        json!({
            "label": label,
            "kind": 2,
            "insertText": format!("{label}(${{1}})"),
            "insertTextFormat": 2,
        })
    }));
    items.extend(function_parameter_names(signature).into_iter().map(|label| {
        json!({ "label": label, "kind": 6, "detail": "function parameter" })
    }));
    items
}

fn function_completion_items_with_locals(signature: &str, source: &str) -> Vec<Value> {
    let mut items = function_completion_items(signature);
    items.extend(function_local_names(source).into_iter().map(|label| {
        json!({ "label": label, "kind": 6, "detail": "local variable" })
    }));
    items
}

fn function_local_names(source: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut tokens = source
        .split(|character: char| !(character == '_' || character.is_alphanumeric()))
        .filter(|token| !token.is_empty());
    while let Some(token) = tokens.next() {
        if token == "var" {
            if let Some(name) = tokens.next() {
                if !names.iter().any(|existing| existing == name) {
                    names.push(name.to_string());
                }
            }
        }
    }
    names
}

fn function_parameter_names(signature: &str) -> Vec<&str> {
    let Some(open) = signature.find('(') else {
        return Vec::new();
    };
    let mut depth = 0_usize;
    let mut close = None;
    for (relative, character) in signature[open..].char_indices() {
        match character {
            '(' => depth = depth.saturating_add(1),
            ')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    close = Some(open + relative);
                    break;
                }
            }
            _ => {}
        }
    }
    let Some(parameters) = close.and_then(|close| signature.get(open + 1..close)) else {
        return Vec::new();
    };
    let mut names = Vec::new();
    let mut parameter_start = 0;
    depth = 0;
    for (index, character) in parameters
        .char_indices()
        .chain(std::iter::once((parameters.len(), ',')))
    {
        match character {
            '(' | '[' | '{' | '<' => depth = depth.saturating_add(1),
            ')' | ']' | '}' | '>' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                if let Some(name) = parameters[parameter_start..index]
                    .split_once(':')
                    .map(|(name, _)| name.trim())
                    .filter(|name| !name.is_empty())
                {
                    names.push(name);
                }
                parameter_start = index + character.len_utf8();
            }
            _ => {}
        }
    }
    names
}

fn collect_value_tokens(value: &CfdValue, c: &mut TokenCollector<'_>) {
    match value {
        CfdValue::Scalar(text, span) => {
            collect_scalar_token(text, *span, c);
        }
        CfdValue::BitExpr(expr) => collect_bit_expr_tokens(expr, c),
        CfdValue::QuotedString(_, span) => c.add_multiline_plain(*span, SEM_STRING),
        CfdValue::FormattedString(value) => collect_formatted_string_tokens(value, c),
        CfdValue::Function(value) => collect_function_tokens(value.span, &value.source, c),
        CfdValue::OptionNone(span) => c.add_plain(*span, SEM_KEYWORD),
        CfdValue::OptionSome(value, span) => {
            c.add_plain(Span::new(span.start, span.start + 4), SEM_KEYWORD);
            collect_value_tokens(value, c);
        }
        CfdValue::ResultOk(value, span) => {
            c.add_plain(Span::new(span.start, span.start + 2), SEM_KEYWORD);
            collect_value_tokens(value, c);
        }
        CfdValue::ResultErr(value, span) => {
            c.add_plain(Span::new(span.start, span.start + 3), SEM_KEYWORD);
            collect_value_tokens(value, c);
        }
        CfdValue::Block(block) => {
            if let Some((_, span)) = &block.type_marker {
                c.add(*span, SEM_TYPE, MOD_REFERENCE | MOD_SCHEMA);
            }
            for field in &block.fields {
                c.add(field.name_span, SEM_PROPERTY, MOD_DECLARATION | MOD_SCHEMA);
                collect_value_tokens(&field.value, c);
            }
        }
        CfdValue::Array(items, _) => {
            for item in items {
                collect_value_tokens(item, c);
            }
        }
        CfdValue::Ref(r) => {
            c.add_plain(Span::new(r.span.start, r.span.start + 1), SEM_OPERATOR);
            if let Some((_, type_span)) = &r.type_name {
                c.add(*type_span, SEM_TYPE, MOD_REFERENCE | MOD_SCHEMA);
            }
            c.add(r.key.1, SEM_NAMESPACE, MOD_REFERENCE | MOD_RECORD);
        }
    }
}

fn collect_formatted_string_tokens(
    value: &coflow_language::CfdFormattedString,
    collector: &mut TokenCollector<'_>,
) {
    let mut cursor = value.span.start;
    for reference in value.segments.iter().filter_map(|segment| match segment {
        CfdFormatSegment::Reference(reference)
            if reference.span.start >= value.span.start
                && reference.span.end <= value.span.end
                && reference.span.start < reference.span.end => Some(reference),
        _ => None,
    }) {
        if reference.span.start > cursor {
            collector.add_multiline_plain(Span::new(cursor, reference.span.start), SEM_STRING);
        }
        collector.add_plain(reference.span, SEM_VARIABLE);
        cursor = reference.span.end;
    }
    if cursor < value.span.end {
        collector.add_multiline_plain(Span::new(cursor, value.span.end), SEM_STRING);
    }
}

fn collect_bit_expr_tokens(expr: &CfdBitExpr, c: &mut TokenCollector<'_>) {
    match &expr.kind {
        CfdBitExprKind::Value(text) => collect_scalar_token(text, expr.span, c),
        CfdBitExprKind::Binary { lhs, rhs, .. } => {
            collect_bit_expr_tokens(lhs, c);
            collect_bit_expr_tokens(rhs, c);
        }
    }
}

fn collect_scalar_token(text: &str, span: Span, c: &mut TokenCollector<'_>) {
    if matches!(text, "None" | "true" | "false") {
        c.add_plain(span, SEM_KEYWORD);
    } else if text
        .bytes()
        .next()
        .is_some_and(|b| b.is_ascii_digit() || b == b'-')
    {
        c.add_plain(span, SEM_NUMBER);
    } else if text.bytes().next().is_some_and(|b| b.is_ascii_uppercase()) {
        c.add(span, SEM_ENUM_MEMBER, MOD_REFERENCE | MOD_SCHEMA);
    }
}

#[allow(clippy::too_many_lines)]
fn collect_function_tokens(span: Span, function_source: &str, c: &mut TokenCollector<'_>) {
    let mut pos = 0;
    while pos < function_source.len() {
        let Some(ch) = function_source[pos..].chars().next() else {
            break;
        };
        if ch.is_whitespace() {
            pos += ch.len_utf8();
            continue;
        }
        if ch == '#' {
            let start = pos;
            while pos < function_source.len() && function_source.as_bytes()[pos] != b'\n' {
                pos += 1;
            }
            c.add_plain(offset_span(span.start, start, pos), SEM_COMMENT);
            continue;
        }
        if ch == '"' {
            let start = pos;
            pos += 1;
            while pos < function_source.len() {
                let Some(current) = function_source[pos..].chars().next() else {
                    break;
                };
                pos += current.len_utf8();
                if current == '\\' {
                    if let Some(escaped) = function_source[pos..].chars().next() {
                        pos += escaped.len_utf8();
                    }
                } else if current == '"' {
                    break;
                }
            }
            c.add_multiline_plain(offset_span(span.start, start, pos), SEM_STRING);
            continue;
        }
        if is_function_ident_start(ch) {
            let start = pos;
            pos += ch.len_utf8();
            while let Some(current) = function_source[pos..].chars().next() {
                if !is_function_ident_continue(current) {
                    break;
                }
                pos += current.len_utf8();
            }
            let text = &function_source[start..pos];
            let previous = previous_non_whitespace(function_source, start);
            let following = next_non_whitespace(function_source, pos);
            let (token_type, modifiers) = if FUNCTION_KEYWORDS.contains(&text) {
                (SEM_KEYWORD, 0)
            } else if FUNCTION_TYPES.contains(&text) {
                (SEM_TYPE, MOD_REFERENCE | MOD_SCHEMA)
            } else if previous == Some('$') {
                (SEM_PROPERTY, MOD_REFERENCE | MOD_SCHEMA)
            } else if previous == Some('&') {
                if function_source[pos..].starts_with("::") {
                    (SEM_TYPE, MOD_REFERENCE | MOD_SCHEMA)
                } else {
                    (SEM_NAMESPACE, MOD_REFERENCE | MOD_RECORD)
                }
            } else if previous == Some('.') {
                if following == Some('(') {
                    (SEM_FUNCTION, MOD_REFERENCE)
                } else {
                    (SEM_PROPERTY, MOD_REFERENCE | MOD_SCHEMA)
                }
            } else if previous_ends_double_colon(function_source, start) {
                if qualified_chain_is_reference(function_source, start) {
                    (SEM_NAMESPACE, MOD_REFERENCE | MOD_RECORD)
                } else {
                    (SEM_ENUM_MEMBER, MOD_REFERENCE | MOD_SCHEMA)
                }
            } else if following == Some(':') {
                (SEM_PARAMETER, MOD_DECLARATION)
            } else if following == Some('(') {
                (SEM_FUNCTION, MOD_REFERENCE)
            } else if text.chars().next().is_some_and(char::is_uppercase) {
                (SEM_TYPE, MOD_REFERENCE | MOD_SCHEMA)
            } else {
                (SEM_VARIABLE, MOD_REFERENCE)
            };
            c.add(offset_span(span.start, start, pos), token_type, modifiers);
            continue;
        }
        if ch.is_ascii_digit() {
            let start = pos;
            pos += 1;
            while pos < function_source.len()
                && function_source.as_bytes()[pos].is_ascii_digit()
            {
                pos += 1;
            }
            if function_source.as_bytes().get(pos) == Some(&b'.')
                && function_source
                    .as_bytes()
                    .get(pos + 1)
                    .is_some_and(u8::is_ascii_digit)
            {
                pos += 1;
                while pos < function_source.len()
                    && function_source.as_bytes()[pos].is_ascii_digit()
                {
                    pos += 1;
                }
            }
            if matches!(function_source.as_bytes().get(pos), Some(b'e' | b'E')) {
                pos += 1;
                if matches!(function_source.as_bytes().get(pos), Some(b'+' | b'-')) {
                    pos += 1;
                }
                while pos < function_source.len()
                    && function_source.as_bytes()[pos].is_ascii_digit()
                {
                    pos += 1;
                }
            }
            c.add_plain(offset_span(span.start, start, pos), SEM_NUMBER);
            continue;
        }

        let operator_len = ["..=", "->", "::", "//", "==", "!=", "<=", ">=", "&&", "||", "=>", "**", "<<", ">>", "..", "+=", "-=", "*=", "/="]
            .iter()
            .find_map(|operator| function_source[pos..].starts_with(operator).then_some(operator.len()))
            .or_else(|| "+-*/%<>=!~&|^?.:$".contains(ch).then_some(ch.len_utf8()));
        if let Some(length) = operator_len {
            c.add_plain(offset_span(span.start, pos, pos + length), SEM_OPERATOR);
            pos += length;
        } else {
            pos += ch.len_utf8();
        }
    }
}

const fn offset_span(base: usize, start: usize, end: usize) -> Span {
    Span::new(base + start, base + end)
}

fn is_function_ident_start(ch: char) -> bool {
    ch == '_' || ch.is_alphabetic()
}

fn is_function_ident_continue(ch: char) -> bool {
    ch == '_' || ch.is_alphanumeric()
}

fn previous_non_whitespace(source: &str, offset: usize) -> Option<char> {
    source[..offset].chars().rev().find(|ch| !ch.is_whitespace())
}

fn next_non_whitespace(source: &str, offset: usize) -> Option<char> {
    source[offset..].chars().find(|ch| !ch.is_whitespace())
}

fn previous_ends_double_colon(source: &str, offset: usize) -> bool {
    source[..offset].trim_end().ends_with("::")
}

fn qualified_chain_is_reference(source: &str, offset: usize) -> bool {
    let prefix = source[..offset].trim_end_matches(':');
    let chain_start = prefix
        .char_indices()
        .rev()
        .find_map(|(index, ch)| {
            (!is_function_ident_continue(ch) && ch != ':' && ch != '&')
                .then_some(index + ch.len_utf8())
        })
        .unwrap_or(0);
    prefix[chain_start..].starts_with('&')
}

fn collect_comment_tokens(source: &str, c: &mut TokenCollector<'_>) {
    let bytes = source.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'#' {
            let start = i;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            c.add_plain(Span::new(start, i), SEM_COMMENT);
        } else if bytes[i] == b'"' {
            // Skip quoted strings so we don't misidentify `//` inside them.
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' {
                    i += 2;
                } else if bytes[i] == b'"' {
                    i += 1;
                    break;
                } else {
                    i += 1;
                }
            }
        } else {
            i += 1;
        }
    }
}

/// Hover: return type info when cursor is on a type name span.
///
/// Returns `Value::Null` when there is nothing to show.
pub fn hover(source: &str, ast: &CfdAst, schema: Option<&CftSchema>, offset: usize) -> Value {
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
            return json!({
                "contents": { "kind": "markdown", "value": detail },
                "range": byte_range(source, record.type_span.start, record.type_span.end),
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
                return json!({
                    "contents": { "kind": "markdown", "value": detail },
                    "range": byte_range(source, field.name_span.start, field.name_span.end),
                });
            }
        }
    }
    Value::Null
}

/// Completion: field names when cursor is inside a record body.
#[cfg(test)]
pub fn completion(source: &str, ast: &CfdAst, schema: Option<&CftSchema>, offset: usize) -> Value {
    completion_with_build(source, ast, schema, None, offset)
}

pub(crate) fn completion_with_build(
    source: &str,
    ast: &CfdAst,
    schema: Option<&CftSchema>,
    build: Option<&LspBuild>,
    offset: usize,
) -> Value {
    if let Some(function) = function_at(ast, offset) {
        let body_start = function.body_span.start.saturating_sub(function.span.start);
        let signature_end = body_start.saturating_sub(1);
        let signature = function.source.get(..signature_end).unwrap_or(&function.source);
        return json!(function_completion_items_with_locals(signature, &function.source));
    }
    let Some(schema) = schema else {
        return json!([]);
    };
    if let Some(items) = formatted_string_completion(source, ast, schema, build, offset) {
        return json!(items);
    }
    if let Some((key, _, group_type)) = incomplete_group_key_at(source, schema, offset) {
        let fields = required_field_snippets(schema, &group_type, 1);
        let body = if fields.is_empty() {
            String::new()
        } else {
            format!("\n  {}\n", fields.join("\n  "))
        };
        return json!([{
            "label": key,
            "kind": 7,
            "detail": format!("new {group_type} record"),
            "insertText": format!("{key} {{{body}}}"),
            "insertTextFormat": 2,
        }]);
    }

    if let Some(context) = completion_context(source, ast, schema, offset) {
        return json!(match context {
            CompletionContext::Value(value_type) => {
                let mut items = selected_flag_values(source, offset, schema, value_type).map_or_else(
                    || value_completion_items(schema, value_type, build),
                    |selected| flag_completion_items(schema, value_type, &selected),
                );
                attach_value_text_edits(source, offset, &mut items);
                items
            }
            CompletionContext::Flags { value_type, selected } => {
                let mut items = flag_completion_items(schema, value_type, &selected);
                attach_value_text_edits(source, offset, &mut items);
                items
            }
            CompletionContext::Fields { type_name, existing } => {
                field_completion_items(source, schema, type_name, &existing, offset)
            }
        });
    }

    for record in &ast.records {
        if !span_contains(record.span, offset) {
            continue;
        }
        let Some(schema_type) = schema.resolve_type(&record.type_name) else {
            continue;
        };
        let existing: std::collections::BTreeSet<&str> =
            record.fields().map(|f| f.name.as_str()).collect();
        let items = field_completion_items(source, schema, &schema_type.name, &existing, offset);
        return json!(items);
    }

    if let Some((type_name, body_start)) = record_context_at(source, schema, offset) {
        let Some(schema_type) = schema.resolve_type(&type_name) else {
            return json!([]);
        };
        let existing = recovered_field_names(source, body_start, offset);
        let items = field_completion_items(source, schema, &schema_type.name, &existing, offset);
        return json!(items);
    }

    // Top-level: suggest known non-abstract type names.
    let types: Vec<Value> = schema
        .all_types()
        .filter(|t| !t.is_abstract)
        .map(|t| {
            let required = required_field_snippets(schema, &t.name, 2);
            let body = if required.is_empty() {
                String::new()
            } else {
                format!("\n  {}\n", required.join("\n  "))
            };
            json!({
                "label": t.name.as_str(),
                "kind": 7,
                "detail": format!("new {} record", t.name),
                "insertText": format!("${{1:key}}: {} {{{body}}}", t.name),
                "insertTextFormat": 2,
            })
        })
        .collect();
    json!(types)
}

fn formatted_string_completion(
    source: &str,
    ast: &CfdAst,
    schema: &CftSchema,
    build: Option<&LspBuild>,
    offset: usize,
) -> Option<Vec<Value>> {
    let record = ast.records.iter().find(|record| {
        record.fields.iter().any(|field| match &field.value {
            CfdValue::FormattedString(value) => span_contains(value.span, offset),
            _ => false,
        })
    })?;
    let schema_type = schema.resolve_type(&record.type_name)?;
    let open = source.get(record.span.start..offset)?.rfind('{')? + record.span.start;
    let prefix = source.get(open + 1..offset)?.trim();
    let parts = prefix.split('.').collect::<Vec<_>>();
    let mut paths = Vec::new();

    if parts.len() >= 2 && schema.resolve_type(parts[0]).is_some() {
        let type_name = parts[0];
        if parts.len() == 2 {
            if let Some(build) = build {
                paths.extend(
                    build
                        .cfd_definitions
                        .keys(schema, type_name)
                        .into_iter()
                        .map(|key| (format!("{type_name}.{key}"), format!("{type_name} record"))),
                );
            }
        } else {
            let path_prefix = parts[..parts.len() - 1].join(".");
            collect_formatted_field_paths(schema, type_name, &path_prefix, 2, &mut paths);
        }
    } else if let Some((owner, path_prefix)) = formatted_path_owner(schema, schema_type.name.as_str(), &parts) {
        collect_formatted_field_paths(schema, owner, &path_prefix, 2, &mut paths);
    } else {
        collect_formatted_field_paths(schema, schema_type.name.as_str(), "", 3, &mut paths);
        paths.extend(schema.all_types().map(|ty| {
            (ty.name.to_string(), "record type".to_string())
        }));
    }

    let range = formatted_reference_range(source, offset);
    Some(
        paths
            .into_iter()
            .map(|(label, detail)| json!({
                "label": label,
                "kind": 5,
                "detail": detail,
                "textEdit": {
                    "range": byte_range(source, range.start, range.end),
                    "newText": label,
                },
            }))
            .collect(),
    )
}

fn collect_formatted_field_paths(
    schema: &CftSchema,
    type_name: &str,
    prefix: &str,
    depth: usize,
    paths: &mut Vec<(String, String)>,
) {
    let Some(schema_type) = schema.resolve_type(type_name) else {
        return;
    };
    for field in schema_type.all_fields() {
        let label = if prefix.is_empty() {
            field.name.to_string()
        } else {
            format!("{prefix}.{}", field.name)
        };
        paths.push((
            label.clone(),
            format!("formatted field: {}", fmt_value_type(&field.value_type)),
        ));
        if depth > 1 {
            if let CftValueType::Object(nested) = &field.value_type {
                collect_formatted_field_paths(schema, nested, &label, depth - 1, paths);
            }
        }
    }
}

fn formatted_path_owner<'a>(
    schema: &'a CftSchema,
    root_type: &'a str,
    parts: &[&str],
) -> Option<(&'a str, String)> {
    if parts.len() < 2 {
        return None;
    }
    let mut owner = root_type;
    for part in &parts[..parts.len() - 1] {
        let field = schema.resolve_type(owner)?.field(part)?;
        let CftValueType::Object(nested) = &field.value_type else {
            return None;
        };
        owner = nested;
    }
    Some((owner, parts[..parts.len() - 1].join(".")))
}

fn formatted_reference_range(source: &str, offset: usize) -> Span {
    let end = offset.min(source.len());
    let start = source[..end]
        .char_indices()
        .rev()
        .take_while(|(_, character)| {
            *character == '_' || *character == '.' || character.is_alphanumeric()
        })
        .map(|(index, _)| index)
        .last()
        .unwrap_or(end);
    Span::new(start, end)
}

enum CompletionContext<'a> {
    Value(&'a CftValueType),
    Flags {
        value_type: &'a CftValueType,
        selected: std::collections::BTreeSet<String>,
    },
    Fields {
        type_name: &'a str,
        existing: std::collections::BTreeSet<&'a str>,
    },
}

fn completion_context<'a>(
    source: &str,
    ast: &'a CfdAst,
    schema: &'a CftSchema,
    offset: usize,
) -> Option<CompletionContext<'a>> {
    for record in &ast.records {
        let Some(schema_type) = schema.resolve_type(&record.type_name) else {
            continue;
        };
        for field in &record.fields {
            if offset >= field.name_span.end && offset <= field.span.end {
                let value_type = schema_type
                    .all_fields()
                    .find(|candidate| candidate.name.as_str() == field.name)
                    .map(|candidate| &candidate.value_type)?;
                return completion_context_in_value(&field.value, schema, value_type, offset);
            }
        }
    }

    let (type_name, body_start) = record_context_at(source, schema, offset)?;
    let field_name = recovered_field_context(source, body_start, offset).1?;
    let value_type = schema
        .resolve_type(&type_name)?
        .all_fields()
        .find(|field| field.name.as_str() == field_name)
        .map(|field| &field.value_type)?;
    Some(CompletionContext::Value(value_type))
}

fn completion_context_in_value<'a>(
    value: &'a CfdValue,
    schema: &'a CftSchema,
    expected: &'a CftValueType,
    offset: usize,
) -> Option<CompletionContext<'a>> {
    match (value, expected) {
        (CfdValue::BitExpr(expression), CftValueType::Enum(_)) => {
            let mut selected = std::collections::BTreeSet::new();
            collect_bit_expr_values(expression, &mut selected);
            Some(CompletionContext::Flags {
                value_type: expected,
                selected,
            })
        }
        (CfdValue::Array(items, _), CftValueType::Array(inner)) => {
            for item in items {
                if offset >= item.span().start && offset <= item.span().end {
                    return completion_context_in_value(item, schema, inner, offset);
                }
            }
            Some(CompletionContext::Value(inner))
        }
        (CfdValue::OptionSome(inner_value, _), CftValueType::Option(inner)) => {
            completion_context_in_value(inner_value, schema, inner, offset)
        }
        (CfdValue::ResultOk(inner_value, _), CftValueType::Result(ok, _)) => {
            completion_context_in_value(inner_value, schema, ok, offset)
        }
        (CfdValue::ResultErr(inner_value, _), CftValueType::Result(_, error)) => {
            completion_context_in_value(inner_value, schema, error, offset)
        }
        (CfdValue::Block(block), CftValueType::Dict(_, value_type)) => {
            for field in &block.fields {
                if offset >= field.name_span.end && offset <= field.span.end {
                    return completion_context_in_value(&field.value, schema, value_type, offset);
                }
            }
            Some(CompletionContext::Value(value_type))
        }
        (CfdValue::Block(block), CftValueType::Object(expected_name)) => {
            let actual_name = block
                .type_marker
                .as_ref()
                .map_or(expected_name.as_str(), |(name, _)| name.as_str());
            let actual_type = schema.resolve_type(actual_name)?;
            for field in &block.fields {
                if offset >= field.name_span.end && offset <= field.span.end {
                    let field_type = actual_type
                        .all_fields()
                        .find(|candidate| candidate.name.as_str() == field.name)
                        .map(|candidate| &candidate.value_type)?;
                    return completion_context_in_value(&field.value, schema, field_type, offset);
                }
            }
            Some(CompletionContext::Fields {
                type_name: actual_type.name.as_str(),
                existing: block.fields.iter().map(|field| field.name.as_str()).collect(),
            })
        }
        _ => Some(CompletionContext::Value(expected)),
    }
}

fn value_completion_items(
    schema: &CftSchema,
    value_type: &CftValueType,
    build: Option<&LspBuild>,
) -> Vec<Value> {
    match value_type {
        CftValueType::Bool => ["true", "false"]
            .into_iter()
            .map(|label| json!({ "label": label, "kind": 14, "detail": "bool" }))
            .collect(),
        CftValueType::Enum(name) => schema.resolve_enum(name.as_str()).map_or_else(Vec::new, |item| {
            item.variants
                .iter()
                .map(|variant| json!({
                    "label": variant.name.as_str(),
                    "kind": 20,
                    "detail": format!("{} enum variant", item.name),
                    "insertText": variant.name.as_str(),
                }))
                .collect()
        }),
        CftValueType::Option(inner) => {
            let mut items = vec![
                json!({ "label": "None", "kind": 14, "detail": value_type.display_label() }),
            ];
            items.push(json!({
                "label": "Some",
                "kind": 3,
                "detail": format!("Some({})", inner.display_label()),
                "insertText": format!("Some(${{1:{}}})", value_placeholder(inner)),
                "insertTextFormat": 2,
            }));
            items.extend(value_completion_items(schema, inner, build));
            items
        }
        CftValueType::Result(ok, error) => vec![
            json!({
                "label": "Ok",
                "kind": 3,
                "detail": format!("Ok({})", ok.display_label()),
                "insertText": format!("Ok(${{1:{}}})", value_placeholder(ok)),
                "insertTextFormat": 2,
            }),
            json!({
                "label": "Err",
                "kind": 3,
                "detail": format!("Err({})", error.display_label()),
                "insertText": format!("Err(${{1:{}}})", value_placeholder(error)),
                "insertTextFormat": 2,
            }),
        ],
        CftValueType::Function(parameters, result) => {
            let parameters = parameters
                .iter()
                .enumerate()
                .map(|(index, parameter)| format!(
                    "{}: {}",
                    parameter.name.as_deref().map_or_else(|| format!("arg{index}"), str::to_string),
                    parameter.value_type.display_label(),
                ))
                .collect::<Vec<_>>()
                .join(", ");
            vec![json!({
                "label": "fn",
                "kind": 3,
                "detail": value_type.display_label(),
                "insertText": format!("fn({parameters}) -> {} {{\n    ${{1}}\n}}", result.display_label()),
                "insertTextFormat": 2,
            })]
        }
        CftValueType::Array(_) => {
            vec![json!({ "label": "[]", "kind": 21, "detail": value_type.display_label() })]
        }
        CftValueType::Dict(_, _) => {
            vec![json!({ "label": "{}", "kind": 21, "detail": value_type.display_label() })]
        }
        CftValueType::Object(name) => {
            let mut items = object_value_completion_items(schema, name);
            items.extend(record_reference_completion_items(schema, name, build, false));
            items
        }
        CftValueType::Unit => {
            vec![json!({ "label": "()", "kind": 21, "detail": "unit" })]
        }
        CftValueType::Int
        | CftValueType::Float
        | CftValueType::String => Vec::new(),
        CftValueType::RecordRef(name) => {
            record_reference_completion_items(schema, name, build, true)
        }
    }
}

fn record_reference_completion_items(
    schema: &CftSchema,
    expected_name: &str,
    build: Option<&LspBuild>,
    include_fallback: bool,
) -> Vec<Value> {
    let keys = build
        .map(|build| build.cfd_definitions.keys(schema, expected_name))
        .unwrap_or_default();
    if keys.is_empty() && include_fallback {
        return vec![json!({
            "label": "record reference",
            "kind": 18,
            "detail": format!("reference to {expected_name}"),
            "insertText": "&${1:key}",
            "insertTextFormat": 2,
        })];
    }
    keys.into_iter()
        .map(|key| json!({
            "label": key,
            "kind": 18,
            "detail": format!("{expected_name} record"),
            "insertText": format!("&{key}"),
        }))
        .collect()
}

fn flag_completion_items(
    schema: &CftSchema,
    value_type: &CftValueType,
    selected: &std::collections::BTreeSet<String>,
) -> Vec<Value> {
    let CftValueType::Enum(name) = value_type else {
        return Vec::new();
    };
    schema.resolve_enum(name.as_str()).map_or_else(Vec::new, |item| {
        item.variants
            .iter()
            .filter(|variant| !selected.contains(variant.name.as_str()))
            .map(|variant| json!({
                "label": variant.name.as_str(),
                "kind": 20,
                "detail": format!("{} flag", item.name),
            }))
            .collect()
    })
}

fn collect_bit_expr_values(
    expression: &CfdBitExpr,
    values: &mut std::collections::BTreeSet<String>,
) {
    match &expression.kind {
        CfdBitExprKind::Value(value) => {
            values.insert(value.clone());
        }
        CfdBitExprKind::Binary { lhs, rhs, .. } => {
            collect_bit_expr_values(lhs, values);
            collect_bit_expr_values(rhs, values);
        }
    }
}

fn selected_flag_values(
    source: &str,
    offset: usize,
    schema: &CftSchema,
    value_type: &CftValueType,
) -> Option<std::collections::BTreeSet<String>> {
    let CftValueType::Enum(name) = value_type else {
        return None;
    };
    let enum_def = schema.resolve_enum(name.as_str())?;
    if !enum_def.is_flag {
        return None;
    }
    let line = source.get(..offset.min(source.len()))?.rsplit('\n').next()?;
    let value = line.rsplit_once(':').map_or(line, |(_, value)| value);
    if !value.contains(['|', '^', '&']) {
        return None;
    }
    Some(
        value
            .split(|character: char| !(character == '_' || character.is_alphanumeric()))
            .filter(|token| {
                enum_def
                    .variants
                    .iter()
                    .any(|variant| variant.name.as_str() == *token)
            })
            .map(str::to_string)
            .collect(),
    )
}

fn object_value_completion_items(schema: &CftSchema, expected_name: &str) -> Vec<Value> {
    schema
        .concrete_assignable_types(expected_name)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|actual_name| {
            let actual = schema.resolve_type(&actual_name)?;
            let fields = required_field_snippets(schema, &actual.name, 1);
            let body = if fields.is_empty() {
                String::new()
            } else {
                format!("\n  {}\n", fields.join("\n  "))
            };
            let marker = (actual.name.as_str() != expected_name || schema.range_is_polymorphic(expected_name))
                .then(|| format!("{} ", actual.name))
                .unwrap_or_default();
            Some(json!({
                "label": actual.name.as_str(),
                "kind": 7,
                "detail": format!("{} object", actual.name),
                "insertText": format!("{marker}{{{body}}}"),
                "insertTextFormat": 2,
            }))
        })
        .collect()
}

fn field_completion_items(
    source: &str,
    schema: &CftSchema,
    type_name: &str,
    existing: &std::collections::BTreeSet<&str>,
    offset: usize,
) -> Vec<Value> {
    let Some(schema_type) = schema.resolve_type(type_name) else {
        return Vec::new();
    };
    let range = identifier_range_before_cursor(source, offset);
    schema_type
        .all_fields()
        .filter(|field| !existing.contains(field.name.as_str()))
        .map(|field| {
            let required = field.default.is_none();
            let new_text = format!(
                "{}: ${{1:{}}}",
                field.name,
                value_placeholder(&field.value_type)
            );
            json!({
                "label": field.name.as_str(),
                "kind": 5,
                "detail": fmt_value_type(&field.value_type),
                "documentation": if required { "Required field" } else { "Field with a schema default" },
                "sortText": format!("{}{}", if required { "0" } else { "1" }, field.name),
                "insertText": new_text,
                "insertTextFormat": 2,
                "textEdit": {
                    "range": byte_range(source, range.start, range.end),
                    "newText": new_text,
                },
            })
        })
        .collect()
}

fn required_field_snippets(schema: &CftSchema, type_name: &str, first_tabstop: usize) -> Vec<String> {
    let Some(schema_type) = schema.resolve_type(type_name) else {
        return Vec::new();
    };
    schema_type
        .all_fields()
        .filter(|field| field.default.is_none())
        .enumerate()
        .map(|(index, field)| {
            format!(
                "{}: ${{{}:{}}},",
                field.name,
                first_tabstop + index,
                value_placeholder(&field.value_type)
            )
        })
        .collect()
}

fn value_placeholder(value_type: &CftValueType) -> String {
    match value_type {
        CftValueType::Int => "0".to_string(),
        CftValueType::Float => "0.0".to_string(),
        CftValueType::Bool => "true".to_string(),
        CftValueType::String => "\"value\"".to_string(),
        CftValueType::Enum(name) => name.to_string(),
        CftValueType::RecordRef(_) => "&key".to_string(),
        CftValueType::Array(_) => "[]".to_string(),
        CftValueType::Dict(_, _) | CftValueType::Object(_) => "{}".to_string(),
        CftValueType::Option(_) => "None".to_string(),
        CftValueType::Result(_, _) => "Ok(value)".to_string(),
        CftValueType::Function(_, _) => "fn() {}".to_string(),
        CftValueType::Unit => "()".to_string(),
    }
}

fn identifier_range_before_cursor(source: &str, offset: usize) -> Span {
    let end = offset.min(source.len());
    let start = source[..end]
        .char_indices()
        .rev()
        .take_while(|(_, character)| *character == '_' || character.is_alphanumeric())
        .map(|(index, _)| index)
        .last()
        .unwrap_or(end);
    Span::new(start, end)
}

fn attach_value_text_edits(source: &str, offset: usize, items: &mut [Value]) {
    let mut range = identifier_range_before_cursor(source, offset);
    if range.start > 0 && source.as_bytes().get(range.start - 1) == Some(&b'&') {
        range.start -= 1;
    }
    for item in items {
        let new_text = item
            .get("insertText")
            .and_then(Value::as_str)
            .or_else(|| item.get("label").and_then(Value::as_str))
            .map(str::to_string);
        let Some(new_text) = new_text else {
            continue;
        };
        if let Value::Object(fields) = item {
            fields.insert("textEdit".to_string(), json!({
                "range": byte_range(source, range.start, range.end),
                "newText": new_text,
            }));
        }
    }
}

fn incomplete_group_keys<'a>(source: &'a str, schema: &CftSchema) -> Vec<(&'a str, Span, String)> {
    let mut keys = Vec::new();
    let mut line_start = 0;
    for line in source.split_inclusive('\n') {
        let content = line.trim_end_matches(['\r', '\n']);
        let leading = content.len() - content.trim_start().len();
        let key = content.trim();
        if coflow_language::is_cft_identifier(key) {
            let span = Span::new(line_start + leading, line_start + leading + key.len());
            if let Some(group_type) = group_type_at(source, schema, span.start) {
                keys.push((key, span, group_type));
            }
        }
        line_start += line.len();
    }
    keys
}

fn incomplete_group_key_at<'a>(
    source: &'a str,
    schema: &CftSchema,
    offset: usize,
) -> Option<(&'a str, Span, String)> {
    let offset = offset.min(source.len());
    let line_start = source[..offset].rfind('\n').map_or(0, |index| index + 1);
    let line_end = source[offset..]
        .find('\n')
        .map_or(source.len(), |index| offset + index);
    let before_cursor = source.get(line_start..offset)?;
    let leading = before_cursor.len() - before_cursor.trim_start().len();
    let key = before_cursor.trim();
    if key.is_empty()
        || !coflow_language::is_cft_identifier(key)
        || !source.get(offset..line_end)?.trim().is_empty()
    {
        return None;
    }
    let span = Span::new(line_start + leading, offset);
    group_type_at(source, schema, span.start).map(|group_type| (key, span, group_type))
}

fn group_type_at(source: &str, schema: &CftSchema, offset: usize) -> Option<String> {
    match brace_context_at(source, schema, offset)? {
        BraceContext::Group(type_name) => Some(type_name),
        BraceContext::Record { .. } | BraceContext::Other => None,
    }
}

fn record_context_at(source: &str, schema: &CftSchema, offset: usize) -> Option<(String, usize)> {
    match brace_context_at(source, schema, offset)? {
        BraceContext::Record {
            type_name,
            body_start,
        } => Some((type_name, body_start)),
        BraceContext::Group(_) | BraceContext::Other => None,
    }
}

#[derive(Clone)]
enum BraceContext {
    Group(String),
    Record {
        type_name: String,
        body_start: usize,
    },
    Other,
}

fn brace_context_at(source: &str, schema: &CftSchema, offset: usize) -> Option<BraceContext> {
    let mut stack: Vec<BraceContext> = Vec::new();
    let mut last_identifier: Option<&str> = None;
    let mut saw_colon = false;
    let mut in_string = false;
    let mut escaped = false;
    let mut line_comment = false;
    let prefix = source.get(..offset.min(source.len()))?;
    let mut characters = prefix.char_indices().peekable();

    while let Some((start, character)) = characters.next() {
        if line_comment {
            if character == '\n' {
                line_comment = false;
            }
            continue;
        }
        if in_string {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_string = false;
            }
            continue;
        }
        if character == '#' {
            line_comment = true;
            continue;
        }
        if character == '/' && characters.peek().is_some_and(|(_, next)| *next == '/') {
            let _ = characters.next();
            line_comment = true;
            continue;
        }
        if character == '"' {
            in_string = true;
            continue;
        }
        if character == '_' || character.is_alphabetic() {
            let mut end = start + character.len_utf8();
            while let Some((next_start, next)) = characters.peek().copied() {
                if next == '_' || next.is_alphanumeric() {
                    let _ = characters.next();
                    end = next_start + next.len_utf8();
                } else {
                    break;
                }
            }
            last_identifier = prefix.get(start..end);
            continue;
        }
        match character {
            ':' => saw_colon = true,
            '{' => {
                let context = match stack.last() {
                    None if !saw_colon => last_identifier
                        .filter(|name| schema.resolve_type(name).is_some())
                        .map_or(BraceContext::Other, |name| {
                            BraceContext::Group(name.to_string())
                        }),
                    None => last_identifier
                        .filter(|name| schema.resolve_type(name).is_some())
                        .map_or(BraceContext::Other, |name| BraceContext::Record {
                            type_name: name.to_string(),
                            body_start: start + 1,
                        }),
                    Some(BraceContext::Group(group_type)) => {
                        let type_name = if saw_colon {
                            last_identifier
                                .filter(|name| schema.resolve_type(name).is_some())
                                .map(str::to_string)
                        } else {
                            Some(group_type.clone())
                        };
                        type_name.map_or(BraceContext::Other, |type_name| BraceContext::Record {
                            type_name,
                            body_start: start + 1,
                        })
                    }
                    Some(BraceContext::Record { .. } | BraceContext::Other) => BraceContext::Other,
                };
                stack.push(context);
                last_identifier = None;
                saw_colon = false;
            }
            '}' => {
                let _ = stack.pop();
                last_identifier = None;
                saw_colon = false;
            }
            ',' | ';' => {
                last_identifier = None;
                saw_colon = false;
            }
            _ if !character.is_whitespace() => last_identifier = None,
            _ => {}
        }
    }

    stack.last().cloned()
}

fn recovered_field_names(
    source: &str,
    body_start: usize,
    offset: usize,
) -> std::collections::BTreeSet<&str> {
    recovered_field_context(source, body_start, offset).0
}

fn recovered_field_context(
    source: &str,
    body_start: usize,
    offset: usize,
) -> (std::collections::BTreeSet<&str>, Option<&str>) {
    let mut names = std::collections::BTreeSet::new();
    let Some(body) = source.get(body_start..offset.min(source.len())) else {
        return (names, None);
    };
    let mut depth = 0_u32;
    let mut identifier: Option<&str> = None;
    let mut current_field: Option<&str> = None;
    let mut expecting_field = true;
    let mut in_string = false;
    let mut escaped = false;
    let mut line_comment = false;
    let mut characters = body.char_indices().peekable();
    while let Some((start, character)) = characters.next() {
        if line_comment {
            if character == '\n' {
                line_comment = false;
            }
            continue;
        }
        if in_string {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_string = false;
            }
            continue;
        }
        if character == '#' {
            line_comment = true;
            continue;
        }
        if character == '"' {
            in_string = true;
            continue;
        }
        if character == '_' || character.is_alphabetic() {
            let mut end = start + character.len_utf8();
            while let Some((next_start, next)) = characters.peek().copied() {
                if next == '_' || next.is_alphanumeric() {
                    let _ = characters.next();
                    end = next_start + next.len_utf8();
                } else {
                    break;
                }
            }
            if depth == 0 && expecting_field {
                identifier = body.get(start..end);
            }
            continue;
        }
        match character {
            '{' | '[' | '(' => depth = depth.saturating_add(1),
            '}' | ']' | ')' => depth = depth.saturating_sub(1),
            ':' if depth == 0 => {
                if let Some(name) = identifier.take() {
                    names.insert(name);
                    current_field = Some(name);
                    expecting_field = false;
                }
            }
            ',' if depth == 0 => {
                identifier = None;
                current_field = None;
                expecting_field = true;
            }
            _ if depth == 0 && expecting_field && !character.is_whitespace() => {
                identifier = None;
            }
            _ => {}
        }
    }
    (names, current_field)
}

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
        CfdValue::OptionSome(value, _) | CfdValue::ResultOk(value, _) | CfdValue::ResultErr(value, _) => {
            function_in_value(value, offset)
        }
        _ => None,
    }
}

/// Definition: return location of the CFT type definition for a `type_span` hit.
///
/// Returns `Value::Null` when nothing is found. The caller must resolve the
/// actual file URI from `schema` module paths.
pub fn definition_type_name(ast: &CfdAst, offset: usize) -> Option<&str> {
    for record in &ast.records {
        if span_contains(record.type_span, offset) {
            return Some(&record.type_name);
        }
        for field in &record.fields {
            if let Some(type_name) = type_name_in_value(&field.value, offset) {
                return Some(type_name);
            }
        }
    }
    None
}

fn type_name_in_value(value: &CfdValue, offset: usize) -> Option<&str> {
    match value {
        CfdValue::Block(block) => {
            if let Some((name, span)) = &block.type_marker {
                if span_contains(*span, offset) {
                    return Some(name.as_str());
                }
            }
            for field in &block.fields {
                if let Some(type_name) = type_name_in_value(&field.value, offset) {
                    return Some(type_name);
                }
            }
            None
        }
        CfdValue::Array(items, _) => {
            for item in items {
                if let Some(type_name) = type_name_in_value(item, offset) {
                    return Some(type_name);
                }
            }
            None
        }
        _ => None,
    }
}

/// Definition: return the owning type and field name when the cursor is on a
/// CFD record field.
pub fn definition_field_name<'a>(
    ast: &'a CfdAst,
    schema: Option<&CftSchema>,
    offset: usize,
) -> Option<(String, &'a str)> {
    for record in &ast.records {
        let type_name = record.type_name.clone();
        for field in &record.fields {
            if let Some(field) = field_name_in_field(field, schema, type_name.clone(), offset) {
                return Some(field);
            }
        }
    }
    None
}

fn field_name_in_field<'a>(
    field: &'a CfdField,
    schema: Option<&CftSchema>,
    owner_type: String,
    offset: usize,
) -> Option<(String, &'a str)> {
    field_name_in_fields(std::slice::from_ref(field), schema, owner_type, offset)
}

fn field_name_in_fields<'a>(
    fields: &'a [CfdField],
    schema: Option<&CftSchema>,
    owner_type: String,
    offset: usize,
) -> Option<(String, &'a str)> {
    for field in fields {
        if span_contains(field.name_span, offset) {
            return Some((owner_type, &field.name));
        }
        let next_owner = schema
            .and_then(|schema| schema.resolve_type(&owner_type))
            .and_then(|ty| {
                ty.all_fields()
                    .find(|schema_field| schema_field.name.as_str() == field.name)
            })
            .and_then(|schema_field| named_type_name(&schema_field.value_type))
            .map(str::to_string);
        if let Some(next_owner) = next_owner {
            if let Some(result) = field_name_in_value(&field.value, schema, next_owner, offset) {
                return Some(result);
            }
        }
    }
    None
}

fn field_name_in_value<'a>(
    value: &'a CfdValue,
    schema: Option<&CftSchema>,
    owner_type: String,
    offset: usize,
) -> Option<(String, &'a str)> {
    match value {
        CfdValue::Block(block) => {
            let owner_type = block
                .type_marker
                .as_ref()
                .map_or(owner_type, |(name, _)| name.clone());
            for field in &block.fields {
                let result = field_name_in_fields(
                    std::slice::from_ref(field),
                    schema,
                    owner_type.clone(),
                    offset,
                );
                if result.is_some() {
                    return result;
                }
            }
            None
        }
        CfdValue::Array(items, _) => {
            for item in items {
                if let Some(result) = field_name_in_value(item, schema, owner_type.clone(), offset)
                {
                    return Some(result);
                }
            }
            None
        }
        _ => None,
    }
}

fn named_type_name(ty: &CftValueType) -> Option<&str> {
    match ty {
        CftValueType::Object(name) => Some(name),
        _ => None,
    }
}

/// Definition: return the expected schema type and key under a reference.
pub fn definition_ref_target(
    ast: &CfdAst,
    schema: Option<&CftSchema>,
    offset: usize,
) -> Option<(String, String)> {
    let schema = schema?;
    for record in &ast.records {
        for field in &record.fields {
            if let Some(target) = ref_target_in_field(field, schema, &record.type_name, offset) {
                return Some(target);
            }
        }
    }
    None
}

fn ref_target_in_field(
    field: &CfdField,
    schema: &CftSchema,
    owner_type: &str,
    offset: usize,
) -> Option<(String, String)> {
    let owner = schema.resolve_type(owner_type)?;
    let field_type = &owner
        .all_fields()
        .find(|candidate| candidate.name.as_str() == field.name)?
        .value_type;
    ref_target_in_value(&field.value, schema, field_type, offset)
}

fn ref_target_in_value(
    value: &CfdValue,
    schema: &CftSchema,
    expected_type: &CftValueType,
    offset: usize,
) -> Option<(String, String)> {
    match value {
        CfdValue::Ref(r) => {
            if span_contains(r.key.1, offset) {
                reference_target_type(expected_type)
                    .map(|target_type| (target_type.to_string(), r.key.0.clone()))
            } else {
                None
            }
        }
        CfdValue::Block(block) => {
            if let CftValueType::Dict(_, value_type) = expected_type {
                for field in &block.fields {
                    if let Some(target) =
                        ref_target_in_value(&field.value, schema, value_type, offset)
                    {
                        return Some(target);
                    }
                }
                return None;
            }
            let owner_type = block
                .type_marker
                .as_ref()
                .map(|(name, _)| name.as_str())
                .or_else(|| reference_target_type(expected_type))?;
            for field in &block.fields {
                if let Some(target) = ref_target_in_field(field, schema, owner_type, offset) {
                    return Some(target);
                }
            }
            None
        }
        CfdValue::Array(items, _) => {
            let CftValueType::Array(item_type) = expected_type else {
                return None;
            };
            for item in items {
                if let Some(target) = ref_target_in_value(item, schema, item_type, offset) {
                    return Some(target);
                }
            }
            None
        }
        CfdValue::OptionSome(value, _) => {
            let CftValueType::Option(inner) = expected_type else {
                return None;
            };
            ref_target_in_value(value, schema, inner, offset)
        }
        CfdValue::ResultOk(value, _) => {
            let CftValueType::Result(ok, _) = expected_type else {
                return None;
            };
            ref_target_in_value(value, schema, ok, offset)
        }
        CfdValue::ResultErr(value, _) => {
            let CftValueType::Result(_, error) = expected_type else {
                return None;
            };
            ref_target_in_value(value, schema, error, offset)
        }
        _ => None,
    }
}

fn reference_target_type(ty: &CftValueType) -> Option<&str> {
    match ty {
        CftValueType::Object(name) | CftValueType::RecordRef(name) => Some(name),
        _ => None,
    }
}

// ── Internal helpers ──────────────────────────────────────────────────────────

fn span_contains(span: Span, offset: usize) -> bool {
    offset >= span.start && offset < span.end.max(span.start + 1)
}

fn fmt_value_type(ty: &CftValueType) -> String {
    ty.display_label()
}

pub fn byte_range(source: &str, start: usize, end: usize) -> Value {
    let s = position_from_byte(source, start);
    let e = position_from_byte(source, end);
    json!({
        "start": { "line": s.0, "character": s.1 },
        "end":   { "line": e.0, "character": e.1 },
    })
}

fn position_from_byte(source: &str, byte_offset: usize) -> (usize, usize) {
    let target = byte_offset.min(source.len());
    let mut line = 0usize;
    let mut character = 0usize;
    for (byte_index, ch) in source.char_indices() {
        if byte_index >= target {
            break;
        }
        if ch == '\n' {
            line += 1;
            character = 0;
        } else {
            character += ch.len_utf16();
        }
    }
    (line, character)
}

struct TokenCollector<'a> {
    source: &'a str,
    tokens: Vec<(usize, usize, u32, u32)>, // (byte_start, byte_end, token_type, modifiers)
}

impl<'a> TokenCollector<'a> {
    const fn new(source: &'a str) -> Self {
        Self {
            source,
            tokens: Vec::new(),
        }
    }

    fn add(&mut self, span: Span, token_type: u32, modifiers: u32) {
        if span.start < span.end {
            self.tokens
                .push((span.start, span.end, token_type, modifiers));
        }
    }

    fn add_plain(&mut self, span: Span, token_type: u32) {
        self.add(span, token_type, 0);
    }

    fn add_multiline_plain(&mut self, span: Span, token_type: u32) {
        let mut start = span.start;
        for line in self.source[span.start..span.end.min(self.source.len())].split_inclusive('\n') {
            let content_len = line.trim_end_matches(['\r', '\n']).len();
            if content_len != 0 {
                self.add_plain(Span::new(start, start + content_len), token_type);
            }
            start += line.len();
        }
    }

    fn into_lsp_data(mut self) -> Value {
        // Sort by start, then remove same-start duplicates and overlapping tokens.
        self.tokens.sort_by_key(|&(start, _, _, _)| start);
        self.tokens.dedup_by_key(|t| t.0);

        let mut data: Vec<u32> = Vec::new();
        let mut prev_line = 0usize;
        let mut prev_char = 0usize;
        let mut prev_end = 0usize; // track end of last emitted token to skip overlaps

        for (start, end, token_type, modifiers) in self.tokens {
            // Skip tokens that overlap with the previous one.
            if start < prev_end {
                continue;
            }
            prev_end = end;
            let (line, character) = position_from_byte(self.source, start);
            let (_, _end_char) = position_from_byte(self.source, end);
            let length_utf16 = self.source[start..end.min(self.source.len())]
                .chars()
                .map(char::len_utf16)
                .sum::<usize>();

            let delta_line = line - prev_line;
            let delta_char = if delta_line == 0 {
                character - prev_char
            } else {
                character
            };

            #[allow(clippy::cast_possible_truncation)]
            {
                data.push(delta_line as u32);
                data.push(delta_char as u32);
                data.push(length_utf16 as u32);
            }
            data.push(token_type);
            data.push(modifiers);

            prev_line = line;
            prev_char = character;
        }

        json!({ "data": data })
    }
}
