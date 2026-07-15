use coflow_cft::CftSchema;
use coflow_data_model::CfdInputValue;
use std::collections::{BTreeMap, BTreeSet};

use super::diagnostics::{
    missing_boundary, reference_needs_marker, syntax, type_mismatch, CellValueDiagnostic,
    CellValueDiagnostics, CellValueErrorCode,
};
use super::markers::{is_type_marker_name, looks_like_bare_record_key};
use super::scan::{find_marker_open_brace, find_top_level_char, split_top_level, strip_outer_pair};
use super::types::{full_fields, FieldMeta};
use super::{parse_value, ValueContext};

pub(super) fn parse_object(
    schema: &CftSchema,
    expected_type: &str,
    text: &str,
    context: ValueContext,
) -> Result<CfdInputValue, CellValueDiagnostics> {
    let expected_fields = full_fields(schema, expected_type)?;
    if text.trim().starts_with('@') {
        return Err(syntax(
            "typed and path references are no longer supported; use `&key`",
        ));
    }
    if text.trim().starts_with('&') {
        return Err(type_mismatch(expected_type));
    }
    if looks_like_bare_record_key(text) {
        return Err(reference_needs_marker(text));
    }

    let ObjectContent {
        actual_type,
        content,
        has_boundary,
    } = object_content(text);
    if !context.is_root() && actual_type.is_none() && !has_boundary {
        return Err(missing_boundary("nested object must use `{}`"));
    }
    if let Some(actual) = &actual_type {
        validate_actual_type(schema, expected_type, actual)?;
    }
    let fields = if let Some(actual) = actual_type.as_deref() {
        full_fields(schema, actual)?
    } else {
        expected_fields
    };
    let content = content.trim();
    if content.is_empty() {
        return Ok(object_value(actual_type, std::iter::empty()));
    }

    let parts = split_top_level(content, ',')?;
    let colon_positions = parts
        .iter()
        .map(|part| find_top_level_char(part, ':'))
        .collect::<Result<Vec<_>, _>>()?;
    let known_field_names = fields
        .iter()
        .map(|field| field.name.as_str())
        .collect::<BTreeSet<_>>();
    let all_parts_have_colon = colon_positions.iter().all(Option::is_some);
    let has_known_named_part = parts.iter().zip(&colon_positions).any(|(part, colon)| {
        colon.is_some_and(|colon| known_field_names.contains(part[..colon].trim()))
    });
    let is_named = all_parts_have_colon;
    let is_mixed = !all_parts_have_colon && has_known_named_part;
    if is_mixed {
        return Err(CellValueDiagnostics {
            diagnostics: vec![CellValueDiagnostic {
                code: CellValueErrorCode::MixedObjectStyle,
                message: "object cannot mix named and positional fields".to_string(),
            }],
        });
    }

    if is_named {
        parse_named_object(schema, actual_type, &fields, &parts, &colon_positions)
    } else {
        parse_positional_object(schema, actual_type, &fields, &parts)
    }
}

fn validate_actual_type(
    schema: &CftSchema,
    expected_type: &str,
    actual_type: &str,
) -> Result<(), CellValueDiagnostics> {
    let Some(actual_schema_type) = schema.resolve_type(actual_type) else {
        return Err(CellValueDiagnostics {
            diagnostics: vec![CellValueDiagnostic {
                code: CellValueErrorCode::UnknownType,
                message: format!("unknown type `{actual_type}`"),
            }],
        });
    };
    if actual_schema_type.is_abstract {
        return Err(CellValueDiagnostics {
            diagnostics: vec![CellValueDiagnostic {
                code: CellValueErrorCode::AbstractObjectType,
                message: format!("abstract type `{actual_type}` cannot be instantiated"),
            }],
        });
    }
    if !schema.is_assignable(actual_type, expected_type) {
        return Err(CellValueDiagnostics {
            diagnostics: vec![CellValueDiagnostic {
                code: CellValueErrorCode::ObjectTypeMismatch,
                message: format!("type `{actual_type}` is not assignable to `{expected_type}`"),
            }],
        });
    }
    Ok(())
}

fn parse_named_object(
    schema: &CftSchema,
    actual_type: Option<String>,
    fields: &[FieldMeta],
    parts: &[&str],
    colon_positions: &[Option<usize>],
) -> Result<CfdInputValue, CellValueDiagnostics> {
    let fields_by_name = fields
        .iter()
        .map(|field| (field.name.as_str(), field))
        .collect::<BTreeMap<_, _>>();
    let mut out = BTreeMap::new();
    let mut seen = BTreeSet::new();
    for (part, colon) in parts.iter().zip(colon_positions) {
        let Some(colon) = colon else {
            continue;
        };
        let name = part[..*colon].trim();
        let value_text = part[*colon + 1..].trim();
        let Some(field) = fields_by_name.get(name) else {
            return Err(CellValueDiagnostics {
                diagnostics: vec![CellValueDiagnostic {
                    code: CellValueErrorCode::UnknownField,
                    message: format!("unknown field `{name}`"),
                }],
            });
        };
        if !seen.insert(name.to_string()) {
            return Err(CellValueDiagnostics {
                diagnostics: vec![CellValueDiagnostic {
                    code: CellValueErrorCode::DuplicateField,
                    message: format!("duplicate field `{name}`"),
                }],
            });
        }
        if value_text == "_" {
            continue;
        }
        if value_text.is_empty() {
            return Err(syntax(format!("field `{name}` has an empty value")));
        }
        out.insert(
            name.to_string(),
            parse_value(schema, &field.ty, value_text, ValueContext::Nested)?,
        );
    }
    Ok(object_value(actual_type, out))
}

fn parse_positional_object(
    schema: &CftSchema,
    actual_type: Option<String>,
    fields: &[FieldMeta],
    parts: &[&str],
) -> Result<CfdInputValue, CellValueDiagnostics> {
    if parts.len() > fields.len() {
        return Err(syntax("too many positional object fields"));
    }

    let mut out = BTreeMap::new();
    for (field, part) in fields.iter().zip(parts) {
        let part = part.trim();
        if part == "_" {
            continue;
        }
        if part.is_empty() {
            return Err(syntax("positional object field has an empty value"));
        }
        out.insert(
            field.name.clone(),
            parse_value(schema, &field.ty, part, ValueContext::Nested)?,
        );
    }
    Ok(object_value(actual_type, out))
}

fn object_value(
    actual_type: Option<String>,
    fields: impl IntoIterator<Item = (String, CfdInputValue)>,
) -> CfdInputValue {
    if let Some(actual_type) = actual_type {
        CfdInputValue::object(actual_type, fields)
    } else {
        CfdInputValue::object_with_declared_type(fields)
    }
}

#[derive(Debug, Clone)]
struct ObjectContent<'a> {
    actual_type: Option<String>,
    content: &'a str,
    has_boundary: bool,
}

fn object_content(text: &str) -> ObjectContent<'_> {
    let text = text.trim();
    if let Some(inner) = strip_outer_pair(text, '{', '}') {
        return ObjectContent {
            actual_type: None,
            content: inner,
            has_boundary: true,
        };
    }

    if let Some(open) = find_marker_open_brace(text) {
        let actual_type = text[..open].trim();
        if is_type_marker_name(actual_type) {
            if let Some(inner) = strip_outer_pair(&text[open..], '{', '}') {
                return ObjectContent {
                    actual_type: Some(actual_type.to_string()),
                    content: inner,
                    has_boundary: true,
                };
            }
        }
    }

    ObjectContent {
        actual_type: None,
        content: text,
        has_boundary: false,
    }
}
