use crate::LoadedValueDraft;
use coflow_language::lexical::record_id_ident_error;

use super::diagnostics::{reference_needs_marker, syntax, type_mismatch, CellValueDiagnostics};
use super::markers::looks_like_bare_record_id;

pub(super) fn parse_ref(
    schema: &coflow_language::cft::CftSchema,
    expected_type: &str,
    text: &str,
) -> Result<LoadedValueDraft, CellValueDiagnostics> {
    let text = text.trim();
    let Some(id) = text.strip_prefix('&') else {
        if text.starts_with('@') {
            return Err(syntax("invalid record reference"));
        }
        if looks_like_bare_record_id(text) {
            return Err(reference_needs_marker(text));
        }
        return Err(type_mismatch(&format!("&{expected_type}")));
    };
    if id.contains('.') || id.contains('[') || id.contains(']') {
        return Err(syntax("invalid record reference"));
    }
    if id.trim() != id {
        return Err(syntax("direct reference id cannot contain whitespace"));
    }
    if id.is_empty() {
        return Err(syntax("reference id is missing"));
    }
    let (explicit_type, id) = id
        .rsplit_once("::")
        .map_or((None, id), |(ty, id)| (Some(ty), id));
    if let Some(type_name) = explicit_type {
        if !schema
            .resolve_type(type_name)
            .is_some_and(coflow_language::cft::CftType::is_table)
            || !schema.is_assignable(type_name, expected_type)
        {
            return Err(type_mismatch(&format!(
                "reference type assignable to &{expected_type}"
            )));
        }
    }
    if let Some(reason) = record_id_ident_error(id) {
        return Err(syntax(format!("invalid reference id `{id}`: {reason}")));
    }
    Ok(explicit_type.map_or_else(
        || LoadedValueDraft::record_ref(id),
        |type_name| LoadedValueDraft::TypedRecordRef {
            type_name: type_name.to_string(),
            id: id.to_string(),
        },
    ))
}
