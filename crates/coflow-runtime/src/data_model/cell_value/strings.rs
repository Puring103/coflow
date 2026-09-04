use super::diagnostics::{syntax, CellValueDiagnostic, CellValueDiagnostics, CellValueErrorCode};
use crate::data_model::{LoadedFieldReference, LoadedFormatSegment, LoadedFormattedString};

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

pub(crate) fn parse_automatic_formatted_string(
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
                    let Some(ch) = rest.chars().next() else {
                        break;
                    };
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
        let Some(ch) = rest.chars().next() else {
            break;
        };
        literal.push(ch);
        pos += ch.len_utf8();
    }
    if !literal.is_empty() {
        segments.push(LoadedFormatSegment::Text(literal));
    }

    Ok(has_reference.then_some(LoadedFormattedString { source, segments }))
}

fn parse_reference(expression: &str) -> Result<LoadedFieldReference, CellValueDiagnostics> {
    let (type_name, key, path) = expression.strip_prefix('&').map_or_else(
        || {
            (
                None,
                None,
                expression
                    .split('.')
                    .map(str::to_string)
                    .collect::<Vec<_>>(),
            )
        },
        |reference| {
            let (type_name, record) = reference
                .split_once("::")
                .map_or((None, reference), |(type_name, record)| {
                    (Some(type_name.to_string()), record)
                });
            let mut parts = record.split('.');
            let key = parts.next().unwrap_or_default();
            let path = parts.map(str::to_string).collect::<Vec<_>>();
            (type_name, Some(key.to_string()), path)
        },
    );
    if path.is_empty()
        || type_name.as_deref().is_some_and(str::is_empty)
        || key.as_deref().is_some_and(str::is_empty)
        || type_name
            .as_deref()
            .is_some_and(|name| !is_reference_name(name))
        || key.as_deref().is_some_and(|name| !is_reference_name(name))
        || path.iter().any(|name| !is_reference_name(name))
        || expression.chars().any(char::is_whitespace)
    {
        return Err(syntax(
            "formatted string reference must use `field`, `&key.field`, or `&Type::key.field`",
        ));
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
