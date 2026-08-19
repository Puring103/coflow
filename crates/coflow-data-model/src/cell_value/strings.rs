use super::diagnostics::{syntax, CellValueDiagnostic, CellValueDiagnostics, CellValueErrorCode};
use crate::{LoadedFieldReference, LoadedFormatSegment, LoadedFormattedString};

pub(super) fn parse_string(text: &str) -> Result<String, CellValueDiagnostics> {
    let text = text.trim();
    if !text.starts_with('"') {
        if string_needs_quotes(text) {
            return Err(CellValueDiagnostics {
                diagnostics: vec![CellValueDiagnostic {
                    code: CellValueErrorCode::StringNeedsQuotes,
                    message: "string value must be quoted".to_string(),
                }],
            });
        }
        return Ok(text.to_string());
    }
    if !text.ends_with('"') || text.len() == 1 {
        return Err(syntax("unterminated string"));
    }
    let mut out = String::new();
    let mut escaped = false;
    for ch in text[1..text.len() - 1].chars() {
        if escaped {
            match ch {
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                other => {
                    return Err(syntax(format!("unsupported string escape `\\{other}`")));
                }
            }
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '"' {
            return Err(syntax("unescaped quote in string"));
        } else {
            out.push(ch);
        }
    }
    if escaped {
        return Err(syntax("unterminated string escape"));
    }
    Ok(out)
}

pub(super) fn parse_formatted_string(
    text: &str,
) -> Result<LoadedFormattedString, CellValueDiagnostics> {
    let text = text.trim();
    if !text.starts_with("f\"") || !text.ends_with('"') || text.len() < 3 {
        return Err(syntax("formatted string must use `f\"...\"`"));
    }
    let mut chars = text[2..text.len() - 1].char_indices().peekable();
    let mut segments = Vec::new();
    let mut literal = String::new();
    while let Some((offset, ch)) = chars.next() {
        match ch {
            '\\' => {
                let Some((_, escaped)) = chars.next() else {
                    return Err(syntax("unterminated formatted string escape"));
                };
                match escaped {
                    '"' => literal.push('"'),
                    '\\' => literal.push('\\'),
                    'n' => literal.push('\n'),
                    'r' => literal.push('\r'),
                    't' => literal.push('\t'),
                    other => return Err(syntax(format!("unsupported string escape `\\{other}`"))),
                }
            }
            '{' if chars.peek().is_some_and(|(_, next)| *next == '{') => {
                let _ = chars.next();
                literal.push('{');
            }
            '}' if chars.peek().is_some_and(|(_, next)| *next == '}') => {
                let _ = chars.next();
                literal.push('}');
            }
            '{' => {
                if !literal.is_empty() {
                    segments.push(LoadedFormatSegment::Text(std::mem::take(&mut literal)));
                }
                let expression_start = offset + ch.len_utf8();
                let mut expression_end = None;
                for (inner_offset, inner) in chars.by_ref() {
                    if inner == '}' {
                        expression_end = Some(inner_offset);
                        break;
                    }
                }
                let Some(expression_end) = expression_end else {
                    return Err(syntax("unterminated formatted string reference"));
                };
                let expression = text[2 + expression_start..2 + expression_end].trim();
                segments.push(LoadedFormatSegment::Reference(parse_reference(expression)?));
            }
            '}' => {
                return Err(syntax(
                    "literal `}` in a formatted string must be written as `}}`",
                ));
            }
            other => literal.push(other),
        }
    }
    if !literal.is_empty() {
        segments.push(LoadedFormatSegment::Text(literal));
    }
    Ok(LoadedFormattedString {
        source: text.to_string(),
        segments,
    })
}

pub(super) fn parse_automatic_formatted_string(
    text: &str,
) -> Result<Option<LoadedFormattedString>, CellValueDiagnostics> {
    let text = text.trim();
    let (source, value) = if text.starts_with('"') {
        (text.to_string(), parse_string(text)?)
    } else {
        (format!("{text:?}"), text.to_string())
    };
    if !value.contains('{') {
        return Ok(None);
    }

    let mut segments = Vec::new();
    let mut literal = String::new();
    let mut pos = 0;
    let mut has_reference = false;
    while pos < value.len() {
        let rest = &value[pos..];
        if rest.starts_with("{{") {
            literal.push('{');
            pos += 2;
            continue;
        }
        if rest.starts_with("}}") {
            literal.push('}');
            pos += 2;
            continue;
        }
        if rest.starts_with('{') {
            let Some(relative_end) = rest.find('}') else {
                break;
            };
            let expression = rest[1..relative_end].trim();
            let reference = match parse_reference(expression) {
                Ok(reference) => reference,
                Err(error) if expression.starts_with('&') => return Err(error),
                Err(_) => {
                    let ch = rest.chars().next().expect("non-empty string remainder");
                    literal.push(ch);
                    pos += ch.len_utf8();
                    continue;
                }
            };
            if !literal.is_empty() {
                segments.push(LoadedFormatSegment::Text(std::mem::take(&mut literal)));
            }
            segments.push(LoadedFormatSegment::Reference(reference));
            has_reference = true;
            pos += relative_end + 1;
            continue;
        }
        let ch = rest.chars().next().expect("non-empty string remainder");
        literal.push(ch);
        pos += ch.len_utf8();
    }
    if !literal.is_empty() {
        segments.push(LoadedFormatSegment::Text(literal));
    }

    Ok(has_reference.then_some(LoadedFormattedString { source, segments }))
}

fn parse_reference(expression: &str) -> Result<LoadedFieldReference, CellValueDiagnostics> {
    let (type_name, key, path) = if let Some(reference) = expression.strip_prefix('&') {
        let (type_name, record) = reference
            .split_once("::")
            .map_or((None, reference), |(type_name, record)| {
                (Some(type_name.to_string()), record)
            });
        let mut parts = record.split('.');
        let key = parts.next().unwrap_or_default();
        let path = parts.map(str::to_string).collect::<Vec<_>>();
        (type_name, Some(key.to_string()), path)
    } else {
        (
            None,
            None,
            expression.split('.').map(str::to_string).collect::<Vec<_>>(),
        )
    };
    if path.is_empty()
        || type_name.as_deref().is_some_and(str::is_empty)
        || key.as_deref().is_some_and(str::is_empty)
        || type_name.as_deref().is_some_and(|name| !is_reference_name(name))
        || key.as_deref().is_some_and(|name| !is_reference_name(name))
        || path.iter().any(|name| !is_reference_name(name))
        || expression.chars().any(char::is_whitespace)
    {
        return Err(syntax("formatted string reference must use `field`, `&key.field`, or `&Type::key.field`"));
    }
    Ok(LoadedFieldReference {
        type_name,
        key,
        path,
    })
}

fn is_reference_name(value: &str) -> bool {
    let mut chars = value.chars();
    chars
        .next()
        .is_some_and(|ch| ch == '_' || ch.is_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_alphanumeric())
}

pub(super) fn string_needs_quotes(text: &str) -> bool {
    text.is_empty()
        || matches!(text, "_" | "null")
        || text
            .chars()
            .any(|ch| matches!(ch, ',' | '|' | ':' | '{' | '}' | '[' | ']'))
}
