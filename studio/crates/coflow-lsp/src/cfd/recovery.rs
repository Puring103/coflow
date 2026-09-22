//! CFD recovery 能力。
use super::{collect_bit_expr_values, CfdAst, CfdValue, CftSchema, CftValueType, Span};

pub(super) enum CompletionContext<'a> {
    Value(&'a CftValueType),
    Flags {
        value_type: &'a CftValueType,
        selected: std::collections::BTreeSet<String>,
    },
    Fields {
        type_name: &'a str,
        existing: std::collections::BTreeSet<&'a str>,
    },
    DimensionFields {
        value_type: &'a CftValueType,
        has_default: bool,
    },
}

pub(super) fn completion_context<'a>(
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

pub(super) fn completion_context_in_value<'a>(
    value: &'a CfdValue,
    schema: &'a CftSchema,
    expected: &'a CftValueType,
    offset: usize,
) -> Option<CompletionContext<'a>> {
    // 非空可选值没有 Some 包装，补全按内部声明类型分析同一语法节点。
    if let CftValueType::Option(inner) = expected {
        if matches!(
            value,
            CfdValue::Block(_)
                | CfdValue::Array(_, _)
                | CfdValue::Ref(_)
                | CfdValue::Function(_)
                | CfdValue::FormattedString(_)
        ) {
            return completion_context_in_value(value, schema, inner, offset);
        }
    }
    match (value, expected) {
        (CfdValue::Dimension(dimension), value_type) => {
            for field in &dimension.fields {
                if offset >= field.name_span.end && offset <= field.span.end {
                    return completion_context_in_value(&field.value, schema, value_type, offset);
                }
            }
            Some(CompletionContext::DimensionFields {
                value_type,
                has_default: dimension.fields.iter().any(|field| field.name == "default"),
            })
        }
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
                existing: block
                    .fields
                    .iter()
                    .map(|field| field.name.as_str())
                    .collect(),
            })
        }
        _ => Some(CompletionContext::Value(expected)),
    }
}

pub(super) fn incomplete_group_keys<'a>(
    source: &'a str,
    schema: &CftSchema,
) -> Vec<(&'a str, Span, String)> {
    let mut keys = Vec::new();
    let mut line_start = 0;
    for line in source.split_inclusive('\n') {
        let content = line.trim_end_matches(['\r', '\n']);
        let leading = content.len() - content.trim_start().len();
        let key = content.trim();
        if coflow_language::lexical::is_cft_identifier(key) {
            let span = Span::new(line_start + leading, line_start + leading + key.len());
            if let Some(group_type) = group_type_at(source, schema, span.start) {
                keys.push((key, span, group_type));
            }
        }
        line_start += line.len();
    }
    keys
}

pub(super) fn incomplete_group_key_at<'a>(
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
        || !coflow_language::lexical::is_cft_identifier(key)
        || !source.get(offset..line_end)?.trim().is_empty()
    {
        return None;
    }
    let span = Span::new(line_start + leading, offset);
    group_type_at(source, schema, span.start).map(|group_type| (key, span, group_type))
}

pub(super) fn group_type_at(source: &str, schema: &CftSchema, offset: usize) -> Option<String> {
    match brace_context_at(source, schema, offset)? {
        BraceContext::Group(type_name) => Some(type_name),
        BraceContext::Record { .. } | BraceContext::Other => None,
    }
}

pub(super) fn record_context_at(
    source: &str,
    schema: &CftSchema,
    offset: usize,
) -> Option<(String, usize)> {
    match brace_context_at(source, schema, offset)? {
        BraceContext::Record {
            type_name,
            body_start,
        } => Some((type_name, body_start)),
        BraceContext::Group(_) | BraceContext::Other => None,
    }
}

#[derive(Clone)]
pub(super) enum BraceContext {
    Group(String),
    Record {
        type_name: String,
        body_start: usize,
    },
    Other,
}

pub(super) fn brace_context_at(
    source: &str,
    schema: &CftSchema,
    offset: usize,
) -> Option<BraceContext> {
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

pub(super) fn recovered_field_names(
    source: &str,
    body_start: usize,
    offset: usize,
) -> std::collections::BTreeSet<&str> {
    recovered_field_context(source, body_start, offset).0
}

pub(super) fn recovered_field_context(
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
