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

// 单元格、默认值和 CFD 共用同一插值解析器，禁止隐式插值和宽松表达式回退。
pub(crate) fn parse_formatted_string(
    text: &str,
) -> Result<LoadedFormattedString, CellValueDiagnostics> {
    let source = text.trim().to_string();
    let (ast, errors) =
        coflow_language::cfd::parse_cfd(&format!("cell: Value {{ field: {source} }}"));
    if let Some(error) = errors.first() {
        return Err(syntax(error.message.clone()));
    }
    let Some(coflow_language::cfd::CfdValue::FormattedString(value)) = ast
        .records
        .first()
        .and_then(|record| record.fields.first())
        .map(|field| &field.value)
    else {
        return Err(syntax("expected an explicit f\"...\" template"));
    };
    let segments = value
        .segments
        .iter()
        .map(|segment| match segment {
            coflow_language::cfd::CfdFormatSegment::Text(text) => {
                LoadedFormatSegment::Text(text.clone())
            }
            coflow_language::cfd::CfdFormatSegment::Reference(reference) => {
                LoadedFormatSegment::Reference(LoadedFieldReference {
                    type_name: reference.type_name.clone(),
                    key: reference.key.clone(),
                    path: reference.path.clone(),
                })
            }
        })
        .collect();
    Ok(LoadedFormattedString { source, segments })
}

pub(super) fn string_needs_quotes(text: &str) -> bool {
    text.is_empty()
        || matches!(text, "_" | "null")
        || text
            .chars()
            .any(|ch| matches!(ch, ',' | '|' | ':' | '{' | '}' | '[' | ']'))
}
