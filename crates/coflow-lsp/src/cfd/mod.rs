//! LSP feature providers for `.cfd` data files.
//!
//! Each function takes the parsed [`CfdAst`] (plus optional compiled schema)
//! and returns a JSON [`Value`] ready to send as an LSP response.

use coflow_cfd::{CfdAst, CfdBlockEntry, CfdRecord, CfdSyntaxDiagnostic, CfdValue};
use coflow_cft::{CftContainer, CftSchemaTypeRef, Span};
use serde_json::{json, Value};

// ── Semantic token type indices (must match SEMANTIC_TOKEN_TYPES in lib.rs) ──

const SEM_NAMESPACE: u32 = 0; // record key
const SEM_TYPE: u32 = 1; // type name
const SEM_ENUM_MEMBER: u32 = 3; // enum variant value
const SEM_PROPERTY: u32 = 4; // field name
const SEM_STRING: u32 = 9; // quoted string value
const SEM_NUMBER: u32 = 8; // numeric scalar
const SEM_COMMENT: u32 = 10; // // and # comments
const SEM_KEYWORD: u32 = 7; // null / true / false
const SEM_OPERATOR: u32 = 11; // @ & ...

const MOD_DECLARATION: u32 = 1 << 0;
const MOD_REFERENCE: u32 = 1 << 1;
const MOD_RECORD: u32 = 1 << 3;
const MOD_SCHEMA: u32 = 1 << 4;

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
                "source": "coflow-cfd",
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
        .fields
        .iter()
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

    // Walk the AST for structured tokens.
    for record in &ast.records {
        collector.add(record.key_span, SEM_NAMESPACE, MOD_DECLARATION | MOD_RECORD);
        collector.add(record.type_span, SEM_TYPE, MOD_REFERENCE | MOD_SCHEMA);
        for entry in &record.entries {
            match entry {
                CfdBlockEntry::Field(field) => {
                    collector.add(field.name_span, SEM_PROPERTY, MOD_DECLARATION | MOD_SCHEMA);
                    collect_value_tokens(&field.value, &mut collector);
                }
                CfdBlockEntry::Spread(value, _) => {
                    collect_value_tokens(value, &mut collector);
                }
            }
        }
    }

    collector.into_lsp_data()
}

fn collect_value_tokens(value: &CfdValue, c: &mut TokenCollector<'_>) {
    match value {
        CfdValue::Scalar(text, span) => {
            if text == "null" || text == "true" || text == "false" {
                c.add_plain(*span, SEM_KEYWORD);
            } else if text
                .bytes()
                .next()
                .is_some_and(|b| b.is_ascii_digit() || b == b'-')
            {
                c.add_plain(*span, SEM_NUMBER);
            } else if text.bytes().next().is_some_and(|b| b.is_ascii_uppercase()) {
                // PascalCase bare identifier → enum variant
                c.add(*span, SEM_ENUM_MEMBER, MOD_REFERENCE | MOD_SCHEMA);
            }
        }
        CfdValue::QuotedString(_, span) => c.add_plain(*span, SEM_STRING),
        CfdValue::Null(span) => c.add_plain(*span, SEM_KEYWORD),
        CfdValue::Block(block) => {
            if let Some((_, span)) = &block.type_marker {
                c.add(*span, SEM_TYPE, MOD_REFERENCE | MOD_SCHEMA);
            }
            for entry in &block.entries {
                match entry {
                    CfdBlockEntry::Field(f) => {
                        c.add(f.name_span, SEM_PROPERTY, MOD_DECLARATION | MOD_SCHEMA);
                        collect_value_tokens(&f.value, c);
                    }
                    CfdBlockEntry::Spread(v, _) => collect_value_tokens(v, c),
                }
            }
        }
        CfdValue::Array(items, _) => {
            for item in items {
                collect_value_tokens(item, c);
            }
        }
        CfdValue::Ref(r) => {
            c.add_plain(Span::new(r.span.start, r.span.start + 1), SEM_OPERATOR);
            c.add(r.key.1, SEM_NAMESPACE, MOD_REFERENCE | MOD_RECORD);
        }
        CfdValue::Spread(inner, span) => {
            c.add_plain(Span::new(span.start, span.start + 3), SEM_OPERATOR); // ...
            collect_value_tokens(inner, c);
        }
    }
}

fn collect_comment_tokens(source: &str, c: &mut TokenCollector<'_>) {
    let bytes = source.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if source[i..].starts_with("//") || bytes[i] == b'#' {
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
pub fn hover(source: &str, ast: &CfdAst, schema: Option<&CftContainer>, offset: usize) -> Value {
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
        for field in &record.fields {
            if span_contains(field.name_span, offset) {
                let detail = schema
                    .and_then(|s| s.resolve_type(&record.type_name))
                    .and_then(|t| t.all_fields.iter().find(|f| f.name == field.name))
                    .map_or_else(
                        || format!("`{}`", field.name),
                        |f| format!("```\n{}: {}\n```", f.name, fmt_type_ref(&f.ty_ref)),
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
pub fn completion(
    _source: &str,
    ast: &CfdAst,
    schema: Option<&CftContainer>,
    offset: usize,
) -> Value {
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
            record.fields.iter().map(|f| f.name.as_str()).collect();
        let items: Vec<Value> = schema_type
            .all_fields
            .iter()
            .filter(|f| !existing.contains(f.name.as_str()))
            .map(|f| {
                json!({
                    "label": f.name,
                    "kind": 5,  // Field
                    "detail": fmt_type_ref(&f.ty_ref),
                })
            })
            .collect();
        return json!(items);
    }

    // Top-level: suggest known non-abstract type names.
    let types: Vec<Value> = schema
        .all_types()
        .filter(|t| !t.is_abstract)
        .map(|t| json!({ "label": t.name, "kind": 7 }))
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
        for entry in &record.entries {
            if let Some(type_name) = type_name_in_entry(entry, offset) {
                return Some(type_name);
            }
        }
    }
    None
}

fn type_name_in_entry(entry: &CfdBlockEntry, offset: usize) -> Option<&str> {
    match entry {
        CfdBlockEntry::Field(field) => type_name_in_value(&field.value, offset),
        CfdBlockEntry::Spread(value, _) => type_name_in_value(value, offset),
    }
}

fn type_name_in_value(value: &CfdValue, offset: usize) -> Option<&str> {
    match value {
        CfdValue::Block(block) => {
            if let Some((name, span)) = &block.type_marker {
                if span_contains(*span, offset) {
                    return Some(name.as_str());
                }
            }
            for entry in &block.entries {
                if let Some(type_name) = type_name_in_entry(entry, offset) {
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
        CfdValue::Spread(inner, _) => type_name_in_value(inner, offset),
        _ => None,
    }
}

/// Definition: return the owning type and field name when the cursor is on a
/// CFD record field.
pub fn definition_field_name<'a>(
    ast: &'a CfdAst,
    schema: Option<&CftContainer>,
    offset: usize,
) -> Option<(String, &'a str)> {
    for record in &ast.records {
        let type_name = record.type_name.clone();
        for entry in &record.entries {
            if let Some(field) = field_name_in_entry(entry, schema, type_name.clone(), offset) {
                return Some(field);
            }
        }
    }
    None
}

fn field_name_in_entry<'a>(
    entry: &'a CfdBlockEntry,
    schema: Option<&CftContainer>,
    owner_type: String,
    offset: usize,
) -> Option<(String, &'a str)> {
    match entry {
        CfdBlockEntry::Field(field) => {
            field_name_in_fields(std::slice::from_ref(field), schema, owner_type, offset)
        }
        CfdBlockEntry::Spread(value, _) => field_name_in_value(value, schema, owner_type, offset),
    }
}

fn field_name_in_fields<'a>(
    fields: &'a [coflow_cfd::CfdField],
    schema: Option<&CftContainer>,
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
                ty.all_fields
                    .iter()
                    .find(|schema_field| schema_field.name == field.name)
            })
            .and_then(|schema_field| named_type_name(&schema_field.ty_ref))
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
    schema: Option<&CftContainer>,
    owner_type: String,
    offset: usize,
) -> Option<(String, &'a str)> {
    match value {
        CfdValue::Block(block) => {
            let owner_type = block
                .type_marker
                .as_ref()
                .map_or(owner_type, |(name, _)| name.clone());
            for entry in &block.entries {
                let result = match entry {
                    CfdBlockEntry::Field(field) => field_name_in_fields(
                        std::slice::from_ref(field),
                        schema,
                        owner_type.clone(),
                        offset,
                    ),
                    CfdBlockEntry::Spread(value, _) => {
                        field_name_in_value(value, schema, owner_type.clone(), offset)
                    }
                };
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
        CfdValue::Spread(inner, _) => field_name_in_value(inner, schema, owner_type, offset),
        _ => None,
    }
}

fn named_type_name(ty: &CftSchemaTypeRef) -> Option<&str> {
    match ty {
        CftSchemaTypeRef::Named(name) => Some(name),
        CftSchemaTypeRef::Nullable(inner) => named_type_name(inner),
        _ => None,
    }
}

/// Definition: return the expected schema type and key under a reference.
pub fn definition_ref_target(
    ast: &CfdAst,
    schema: Option<&CftContainer>,
    offset: usize,
) -> Option<(String, String)> {
    let schema = schema?;
    for record in &ast.records {
        for entry in &record.entries {
            if let Some(target) = ref_target_in_entry(entry, schema, &record.type_name, offset) {
                return Some(target);
            }
        }
    }
    None
}

fn ref_target_in_entry(
    entry: &CfdBlockEntry,
    schema: &CftContainer,
    owner_type: &str,
    offset: usize,
) -> Option<(String, String)> {
    match entry {
        CfdBlockEntry::Field(field) => {
            let owner = schema.resolve_type(owner_type)?;
            let field_type = &owner
                .all_fields
                .iter()
                .find(|candidate| candidate.name == field.name)?
                .ty_ref;
            ref_target_in_value(&field.value, schema, field_type, offset)
        }
        CfdBlockEntry::Spread(value, _) => ref_target_in_value(
            value,
            schema,
            &CftSchemaTypeRef::Ref(owner_type.to_string()),
            offset,
        ),
    }
}

fn ref_target_in_value(
    value: &CfdValue,
    schema: &CftContainer,
    expected_type: &CftSchemaTypeRef,
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
            let expected_type = strip_nullable(expected_type);
            if let CftSchemaTypeRef::Dict(_, value_type) = expected_type {
                for entry in &block.entries {
                    let value = match entry {
                        CfdBlockEntry::Field(field) => &field.value,
                        CfdBlockEntry::Spread(value, _) => value,
                    };
                    if let Some(target) = ref_target_in_value(value, schema, value_type, offset) {
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
            for entry in &block.entries {
                if let Some(target) = ref_target_in_entry(entry, schema, owner_type, offset) {
                    return Some(target);
                }
            }
            None
        }
        CfdValue::Array(items, _) => {
            let CftSchemaTypeRef::Array(item_type) = strip_nullable(expected_type) else {
                return None;
            };
            for item in items {
                if let Some(target) = ref_target_in_value(item, schema, item_type, offset) {
                    return Some(target);
                }
            }
            None
        }
        CfdValue::Spread(inner, _) => ref_target_in_value(inner, schema, expected_type, offset),
        _ => None,
    }
}

fn strip_nullable(ty: &CftSchemaTypeRef) -> &CftSchemaTypeRef {
    match ty {
        CftSchemaTypeRef::Nullable(inner) => strip_nullable(inner),
        _ => ty,
    }
}

fn reference_target_type(ty: &CftSchemaTypeRef) -> Option<&str> {
    match strip_nullable(ty) {
        CftSchemaTypeRef::Named(name) | CftSchemaTypeRef::Ref(name) => Some(name),
        _ => None,
    }
}

// ── Internal helpers ──────────────────────────────────────────────────────────

fn span_contains(span: Span, offset: usize) -> bool {
    offset >= span.start && offset < span.end.max(span.start + 1)
}

fn fmt_type_ref(ty: &CftSchemaTypeRef) -> String {
    match ty {
        CftSchemaTypeRef::Int => "int".to_string(),
        CftSchemaTypeRef::Float => "float".to_string(),
        CftSchemaTypeRef::Bool => "bool".to_string(),
        CftSchemaTypeRef::String => "string".to_string(),
        CftSchemaTypeRef::Named(name) => name.clone(),
        CftSchemaTypeRef::Ref(name) => format!("&{name}"),
        CftSchemaTypeRef::Array(inner) => format!("[{}]", fmt_type_ref(inner)),
        CftSchemaTypeRef::Dict(k, v) => {
            format!("{{{}: {}}}", fmt_type_ref(k), fmt_type_ref(v))
        }
        CftSchemaTypeRef::Nullable(inner) => format!("{}?", fmt_type_ref(inner)),
    }
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
