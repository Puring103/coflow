use crate::LoadedValueDraft;
use coflow_language::lexical::record_key_ident_error;

use super::diagnostics::{reference_needs_marker, syntax, type_mismatch, CellValueDiagnostics};
use super::markers::looks_like_bare_record_key;

pub(super) fn parse_ref(
    schema: &coflow_language::cft::CftSchema,
    expected_type: &str,
    text: &str,
) -> Result<LoadedValueDraft, CellValueDiagnostics> {
    let text = text.trim();
    let Some(key) = text.strip_prefix('&') else {
        if text.starts_with('@') {
            return Err(syntax("invalid record reference"));
        }
        if looks_like_bare_record_key(text) {
            return Err(reference_needs_marker(text));
        }
        return Err(type_mismatch(&format!("&{expected_type}")));
    };
    if key.contains('.') || key.contains('[') || key.contains(']') {
        return Err(syntax("invalid record reference"));
    }
    if key.trim() != key {
        return Err(syntax("direct reference key cannot contain whitespace"));
    }
    if key.is_empty() {
        return Err(syntax("reference key is missing"));
    }
    let (explicit_type, key) = key
        .rsplit_once("::")
        .map_or((None, key), |(ty, key)| (Some(ty), key));
    if let Some(type_name) = explicit_type {
        if !schema
            .resolve_type(type_name)
            .is_some_and(|ty| ty.is_table())
            || !schema.is_assignable(type_name, expected_type)
        {
            return Err(type_mismatch(&format!(
                "reference type assignable to &{expected_type}"
            )));
        }
    }
    if let Some(reason) = record_key_ident_error(key) {
        return Err(syntax(format!("invalid reference key `{key}`: {reason}")));
    }
    Ok(if let Some(type_name) = explicit_type {
        LoadedValueDraft::TypedRecordRef {
            type_name: type_name.to_string(),
            key: key.to_string(),
        }
    } else {
        LoadedValueDraft::record_ref(key)
    })
}
