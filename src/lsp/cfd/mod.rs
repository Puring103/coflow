//! LSP feature providers for `.cfd` data files.
//!
//! Each function takes the parsed [`CfdAst`] (plus optional compiled schema)
//! and returns a JSON [`Value`] ready to send as an LSP response.

use coflow_language::cfd::{
    CfdAst, CfdBitExpr, CfdBitExprKind, CfdField, CfdRecord, CfdSyntaxDiagnostic, CfdValue,
};
use coflow_language::{CftSchema, CftValueType, Span};
use serde_json::{json, Value};

use super::semantic_tokens::{
    MOD_DECLARATION, MOD_RECORD, MOD_REFERENCE, MOD_SCHEMA, SEM_COMMENT, SEM_ENUM_MEMBER,
    SEM_FUNCTION, SEM_KEYWORD, SEM_NAMESPACE, SEM_NUMBER, SEM_OPERATOR, SEM_PARAMETER,
    SEM_PROPERTY, SEM_STRING, SEM_TYPE, SEM_VARIABLE,
};

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
pub fn semantic_tokens(source: &str, ast: &CfdAst) -> Value {
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

    collector.into_lsp_data()
}

fn collect_value_tokens(value: &CfdValue, c: &mut TokenCollector<'_>) {
    match value {
        CfdValue::Scalar(text, span) => {
            collect_scalar_token(text, *span, c);
        }
        CfdValue::BitExpr(expr) => collect_bit_expr_tokens(expr, c),
        CfdValue::QuotedString(_, span) => c.add_multiline_plain(*span, SEM_STRING),
        CfdValue::FormattedString(value) => c.add_multiline_plain(value.span, SEM_STRING),
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
            let (token_type, modifiers) = if matches!(
                text,
                "fn" | "var" | "return" | "if" | "else" | "match" | "for" | "while"
                    | "break" | "continue" | "in" | "is" | "true" | "false" | "None"
                    | "Some" | "Ok" | "Err"
            ) {
                (SEM_KEYWORD, 0)
            } else if matches!(text, "int" | "float" | "bool" | "string" | "Option" | "Result") {
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
pub fn completion(_source: &str, ast: &CfdAst, schema: Option<&CftSchema>, offset: usize) -> Value {
    let Some(schema) = schema else {
        return json!([]);
    };

    for record in &ast.records {
        if !span_contains(record.span, offset) {
            continue;
        }
        let Some(schema_type) = schema.resolve_type(&record.type_name) else {
            continue;
        };
        let existing: std::collections::BTreeSet<&str> =
            record.fields().map(|f| f.name.as_str()).collect();
        let items: Vec<Value> = schema_type
            .all_fields()
            .filter(|f| !existing.contains(f.name.as_str()))
            .map(|f| {
                json!({
                    "label": f.name.as_str(),
                    "kind": 5,  // Field
                    "detail": fmt_value_type(&f.value_type),
                })
            })
            .collect();
        return json!(items);
    }

    // Top-level: suggest known non-abstract type names.
    let types: Vec<Value> = schema
        .all_types()
        .filter(|t| !t.is_abstract)
        .map(|t| json!({ "label": t.name.as_str(), "kind": 7 }))
        .collect();
    json!(types)
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
