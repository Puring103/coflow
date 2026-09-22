use super::diagnostics::{syntax, CellValueDiagnostic, CellValueDiagnostics, CellValueErrorCode};
use crate::CallableSource;

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
    coflow_language::lexical::decode_string(text).map_err(|error| syntax(error.message))
}

pub(crate) fn parse_automatic_formatted_string(
    text: &str,
) -> Result<Option<CallableSource>, CellValueDiagnostics> {
    let text = text.trim();
    if !text.starts_with("f\"") {
        return Ok(None);
    }
    coflow_language::lexical::validate_formatted_string_literal(text)
        .map_err(|error| syntax(error.message))?;
    Ok(Some(CallableSource {
        from_default: false,
        location: None,
        imports: Default::default(),
        constant_origin: None,
        source: text.to_string(),
    }))
}

pub(super) fn string_needs_quotes(text: &str) -> bool {
    text.is_empty()
        || matches!(text, "_" | "null")
        || text
            .chars()
            .any(|ch| matches!(ch, ',' | '|' | ':' | '{' | '}' | '[' | ']'))
}
