use coflow_cft::{
    record_key_ident_error, CftContainer, CftFieldMeta, CftSchemaTypeRef, CftSchemaView,
};
use coflow_data_model::CfdInputValue;
use std::collections::BTreeMap;

use crate::{CfdTextDiagnostic, CfdTextDiagnostics, CfdTextErrorCode, CfdTextSpan};

#[derive(Debug, Clone)]
pub(super) struct FieldMeta {
    pub(super) name: String,
    pub(super) ty: CftSchemaTypeRef,
}

#[derive(Debug, Clone)]
pub(super) struct ParsedObjectFields {
    pub(super) spreads: Vec<CfdInputValue>,
    pub(super) fields: BTreeMap<String, CfdInputValue>,
}

pub(super) fn validate_record_key(key: &str, pos: usize) -> Result<(), CfdTextDiagnostics> {
    if let Some(reason) = record_key_ident_error(key) {
        return Err(error_at(
            pos,
            CfdTextErrorCode::Syntax,
            format!("invalid record key `{key}`: {reason}"),
        ));
    }
    Ok(())
}

pub(super) fn validate_group_type(
    schema: &CftContainer,
    type_name: &str,
    pos: usize,
) -> Result<(), CfdTextDiagnostics> {
    if schema.resolve_type(type_name).is_none() {
        return Err(error_at(
            pos,
            CfdTextErrorCode::UnknownType,
            format!("unknown type `{type_name}`"),
        ));
    }
    Ok(())
}

pub(super) fn validate_record_type(
    schema: &CftContainer,
    actual_type: &str,
    pos: usize,
) -> Result<(), CfdTextDiagnostics> {
    let Some(schema_type) = schema.resolve_type(actual_type) else {
        return Err(error_at(
            pos,
            CfdTextErrorCode::UnknownType,
            format!("unknown type `{actual_type}`"),
        ));
    };
    if schema_type.is_abstract {
        return Err(error_at(
            pos,
            CfdTextErrorCode::AbstractObjectType,
            format!("abstract type `{actual_type}` cannot be instantiated"),
        ));
    }
    Ok(())
}

pub(super) fn validate_actual_type(
    schema: &CftContainer,
    expected_type: &str,
    actual_type: &str,
    pos: usize,
) -> Result<(), CfdTextDiagnostics> {
    let Some(schema_type) = schema.resolve_type(actual_type) else {
        return Err(error_at(
            pos,
            CfdTextErrorCode::UnknownType,
            format!("unknown type `{actual_type}`"),
        ));
    };
    if schema_type.is_abstract {
        return Err(error_at(
            pos,
            CfdTextErrorCode::AbstractObjectType,
            format!("abstract type `{actual_type}` cannot be instantiated"),
        ));
    }
    if !schema.is_assignable(actual_type, expected_type) {
        return Err(error_at(
            pos,
            CfdTextErrorCode::ObjectTypeMismatch,
            format!("type `{actual_type}` is not assignable to `{expected_type}`"),
        ));
    }
    Ok(())
}

pub(super) fn full_fields(
    schema: &CftContainer,
    type_name: &str,
) -> Result<Vec<FieldMeta>, CfdTextDiagnostics> {
    let view = CftSchemaView::new(schema);
    let Some(fields) = view.fields(type_name) else {
        return Err(CfdTextDiagnostics::one(CfdTextDiagnostic::error(
            CfdTextErrorCode::UnknownType,
            format!("unknown type `{type_name}`"),
            CfdTextSpan::default(),
        )));
    };
    Ok(fields.map(field_meta).collect::<Vec<_>>())
}

fn field_meta(field: &CftFieldMeta) -> FieldMeta {
    FieldMeta {
        name: field.name.clone(),
        ty: field.ty_ref.clone(),
    }
}

fn error_at(pos: usize, code: CfdTextErrorCode, message: impl Into<String>) -> CfdTextDiagnostics {
    CfdTextDiagnostics::one(CfdTextDiagnostic::error(
        code,
        message,
        CfdTextSpan {
            start: pos,
            end: pos,
        },
    ))
}
