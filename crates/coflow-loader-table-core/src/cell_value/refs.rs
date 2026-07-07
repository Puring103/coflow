use coflow_cft::record_key_ident_error;
use coflow_data_model::CfdInputValue;

use super::diagnostics::{reference_needs_marker, syntax, type_mismatch, CellValueDiagnostics};
use super::markers::looks_like_bare_record_key;

pub(super) fn parse_ref(
    expected_type: &str,
    text: &str,
) -> Result<CfdInputValue, CellValueDiagnostics> {
    let text = text.trim();
    let Some(key) = text.strip_prefix('&') else {
        if text.starts_with('@') {
            return Err(syntax(
                "typed and path references are no longer supported; use `&key`",
            ));
        }
        if looks_like_bare_record_key(text) {
            return Err(reference_needs_marker(text));
        }
        return Err(type_mismatch(&format!("&{expected_type}")));
    };
    if key.contains('.') || key.contains('[') || key.contains(']') {
        return Err(syntax("record references do not support paths"));
    }
    if key.trim() != key {
        return Err(syntax("direct reference key cannot contain whitespace"));
    }
    if key.is_empty() {
        return Err(syntax("reference key is missing"));
    }
    if let Some(reason) = record_key_ident_error(key) {
        return Err(syntax(format!("invalid reference key `{key}`: {reason}")));
    }
    Ok(CfdInputValue::record_ref(key))
}
