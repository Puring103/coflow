use coflow_api::{Diagnostic, DiagnosticSet, Severity, WriteFieldPathSegment};
use coflow_cft::{CftSchema, CftValueType, TypeName};
use coflow_data_model::{
    CfdPath, CfdPathSegment, CfdRecordId, CfdValue, CfdValueSemanticContext, ValueValidationMode,
    ValueValidationRequest,
};
use std::collections::BTreeMap;

use crate::ProjectSession;

fn validate_record_key_for_stage(
    key: &str,
    code: &'static str,
    stage: &'static str,
) -> Result<(), DiagnosticSet> {
    if let Some(reason) = coflow_cft::record_key_ident_error(key) {
        return Err(one_error(
            code,
            stage,
            format!("record key `{key}` is invalid: {reason}"),
        ));
    }
    Ok(())
}

pub fn ensure_record_key_available_with_conflict_code(
    session: &ProjectSession,
    actual_type: &str,
    key: &str,
    current_record: Option<CfdRecordId>,
    code: &'static str,
    conflict_code: &'static str,
    stage: &'static str,
) -> Result<(), DiagnosticSet> {
    validate_record_key_for_stage(key, code, stage)?;
    let Some(inheritance_root) = session.schema.inheritance_root(actual_type) else {
        return Err(one_error(
            code,
            stage,
            format!("unknown type `{actual_type}`"),
        ));
    };
    let Some(existing_id) = session.model.record_by_domain_key(inheritance_root, key) else {
        return Ok(());
    };
    if current_record == Some(existing_id) {
        return Ok(());
    }
    let existing = session.model.record(existing_id).ok_or_else(|| {
        one_error(
            conflict_code,
            stage,
            format!("key `{key}` already exists in `{actual_type}` inheritance domain"),
        )
    })?;
    Err(one_error(
        conflict_code,
        stage,
        format!(
            "key `{key}` already exists in `{actual_type}` inheritance domain as `{}.{}`",
            existing.actual_type(),
            existing.key
        ),
    ))
}

pub(crate) fn expected_type_for_write_path(
    schema: &CftSchema,
    actual_type: &str,
    path: &[WriteFieldPathSegment],
    code: &'static str,
    stage: &'static str,
) -> Result<CftValueType, DiagnosticSet> {
    let cfd_path = write_path_to_cfd_path(path, code, stage)?;
    expected_type_for_cfd_path(schema, actual_type, &cfd_path.segments, code, stage)
}

pub(crate) fn validate_value_at_write_path(
    session: &ProjectSession,
    actual_type: &str,
    path: &[WriteFieldPathSegment],
    value: &CfdValue,
    code: &'static str,
    stage: &'static str,
) -> Result<(), DiagnosticSet> {
    let schema = session.schema();
    let expected = expected_type_for_write_path(schema, actual_type, path, code, stage)?;
    validate_value_for_write(session, schema, &expected, value, code, stage)
}

pub(crate) fn expected_type_for_cfd_path(
    schema: &CftSchema,
    actual_type: &str,
    path: &[CfdPathSegment],
    code: &'static str,
    stage: &'static str,
) -> Result<CftValueType, DiagnosticSet> {
    if path.is_empty() {
        return Err(one_error(code, stage, "field path must not be empty"));
    }
    let Some(root_type) = schema.resolve_type(actual_type) else {
        return Err(one_error(
            code,
            stage,
            format!("unknown type `{actual_type}`"),
        ));
    };
    let mut current = CftValueType::Object(root_type.name.clone());
    for segment in path {
        current = match segment {
            CfdPathSegment::Field(field) => {
                let CftValueType::Object(type_name) = non_nullable(&current) else {
                    return Err(one_error(
                        code,
                        stage,
                        format!("field `{field}` cannot be selected from this value"),
                    ));
                };
                schema
                    .field(type_name, field)
                    .map(|field| field.value_type.clone())
                    .ok_or_else(|| {
                        if schema.resolve_type(type_name).is_none() {
                            return one_error(code, stage, format!("unknown type `{type_name}`"));
                        }
                        one_error(
                            code,
                            stage,
                            format!("unknown field `{field}` on type `{type_name}`"),
                        )
                    })?
            }
            CfdPathSegment::Index(index) => {
                let CftValueType::Array(inner) = non_nullable(&current) else {
                    return Err(one_error(
                        code,
                        stage,
                        format!("array index `{index}` cannot be selected from this value"),
                    ));
                };
                (**inner).clone()
            }
            CfdPathSegment::DictKey(key) => {
                let CftValueType::Dict(_, item) = non_nullable(&current) else {
                    return Err(one_error(
                        code,
                        stage,
                        format!("dict key `{key}` cannot be selected from this value"),
                    ));
                };
                (**item).clone()
            }
        };
    }
    Ok(current)
}

pub(crate) fn validate_value_for_write(
    session: &ProjectSession,
    schema: &CftSchema,
    expected: &CftValueType,
    value: &CfdValue,
    code: &'static str,
    stage: &'static str,
) -> Result<(), DiagnosticSet> {
    validate_value_semantics(
        session,
        schema,
        ValueValidationRequest::new(expected, value, ValueValidationMode::Mutation),
        None,
        code,
        code,
        stage,
    )
}

pub(crate) fn validate_value_semantics(
    session: &ProjectSession,
    schema: &CftSchema,
    request: ValueValidationRequest<'_>,
    pending_records: Option<&BTreeMap<crate::RecordCoordinate, usize>>,
    value_code: &'static str,
    reference_code: &'static str,
    stage: &'static str,
) -> Result<(), DiagnosticSet> {
    let context = ProjectValueSemanticContext {
        session,
        pending_records,
    };
    coflow_data_model::validate_value_for_schema(schema, &context, request).map_err(|err| {
        let reference_error = matches!(
            err.kind(),
            coflow_data_model::CfdValueSemanticErrorKind::RefTargetNotFound
                | coflow_data_model::CfdValueSemanticErrorKind::RefTargetTypeMismatch
                | coflow_data_model::CfdValueSemanticErrorKind::MissingRequiredField
        );
        let code = if reference_error {
            reference_code
        } else {
            value_code
        };
        let message = if code == "MUTATION-VALUE"
            && err.kind() == coflow_data_model::CfdValueSemanticErrorKind::TypeMismatch
        {
            format!(
                "value does not match expected schema type: {}",
                err.message()
            )
        } else {
            err.message().to_string()
        };
        one_error(code, stage, message)
    })
}

pub(crate) fn ensure_object_type_assignable(
    schema: &CftSchema,
    expected_type: &str,
    actual_type: &str,
    code: &'static str,
    stage: &'static str,
) -> Result<(), DiagnosticSet> {
    coflow_data_model::validate_object_type_assignable(schema, expected_type, actual_type)
        .map_err(|err| one_error(code, stage, err.message()))
}

struct ProjectValueSemanticContext<'a> {
    session: &'a ProjectSession,
    pending_records: Option<&'a BTreeMap<crate::RecordCoordinate, usize>>,
}

impl CfdValueSemanticContext for ProjectValueSemanticContext<'_> {
    fn record_by_domain_key(&self, inheritance_root: &TypeName, key: &str) -> Option<CfdRecordId> {
        self.session
            .model
            .record_by_domain_key(inheritance_root, key)
    }

    fn record_actual_type(&self, id: CfdRecordId) -> Option<&str> {
        self.session
            .model
            .record(id)
            .map(coflow_data_model::CfdRecord::actual_type)
    }

    fn pending_record_actual_type(&self, inheritance_root: &TypeName, key: &str) -> Option<&str> {
        self.pending_records?
            .keys()
            .find(|record| {
                record.key() == key
                    && self.session.schema.inheritance_root(&record.actual_type)
                        == Some(inheritance_root)
            })
            .map(|record| record.actual_type.as_str())
    }
}

pub fn write_path_to_cfd_path(
    path: &[WriteFieldPathSegment],
    code: &'static str,
    stage: &'static str,
) -> Result<CfdPath, DiagnosticSet> {
    if path.is_empty() {
        return Err(one_error(code, stage, "field path must not be empty"));
    }
    Ok(CfdPath {
        segments: path.to_vec(),
    })
}

fn non_nullable(ty: &CftValueType) -> &CftValueType {
    match ty {
        CftValueType::Nullable(inner) => non_nullable(inner),
        other => other,
    }
}

fn one_error(code: &'static str, stage: &'static str, message: impl Into<String>) -> DiagnosticSet {
    DiagnosticSet::one(Diagnostic {
        code: code.to_string(),
        stage: stage.to_string(),
        severity: Severity::Error,
        message: message.into(),
        primary: None,
        related: Vec::new(),
    })
}
