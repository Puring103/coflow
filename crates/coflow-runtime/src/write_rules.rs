use coflow_api::{Diagnostic, DiagnosticSet, Severity, WriteFieldPathSegment};
use coflow_cft::{CftSchemaTypeRef, CompiledSchema};
use coflow_data_model::{
    CfdDomainId, CfdPath, CfdPathSegment, CfdRecordId, CfdValue, CfdValueSemanticContext,
    PendingInsertRef,
};

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
    let Some(domain) = session.model.type_domain_id(actual_type) else {
        return Err(one_error(
            code,
            stage,
            format!("unknown type `{actual_type}`"),
        ));
    };
    let Some(existing_id) = session.model.record_by_domain_key(domain, key) else {
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
    schema: &CompiledSchema,
    actual_type: &str,
    path: &[WriteFieldPathSegment],
    code: &'static str,
    stage: &'static str,
) -> Result<CftSchemaTypeRef, DiagnosticSet> {
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
    let schema = session.compiled_schema();
    let expected = expected_type_for_write_path(schema, actual_type, path, code, stage)?;
    validate_value_for_write(session, schema, &expected, value, code, stage)
}

pub(crate) fn expected_type_for_cfd_path(
    schema: &CompiledSchema,
    actual_type: &str,
    path: &[CfdPathSegment],
    code: &'static str,
    stage: &'static str,
) -> Result<CftSchemaTypeRef, DiagnosticSet> {
    if path.is_empty() {
        return Err(one_error(code, stage, "field path must not be empty"));
    }
    let mut current = CftSchemaTypeRef::Named(actual_type.to_string());
    for segment in path {
        current = match segment {
            CfdPathSegment::Field(field) => {
                let CftSchemaTypeRef::Named(type_name) = non_nullable(&current) else {
                    return Err(one_error(
                        code,
                        stage,
                        format!("field `{field}` cannot be selected from this value"),
                    ));
                };
                schema
                    .field_type(type_name, field)
                    .cloned()
                    .ok_or_else(|| {
                        if !schema.has_type(type_name) {
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
                let CftSchemaTypeRef::Array(inner) = non_nullable(&current) else {
                    return Err(one_error(
                        code,
                        stage,
                        format!("array index `{index}` cannot be selected from this value"),
                    ));
                };
                (**inner).clone()
            }
            CfdPathSegment::DictKey(key) => {
                let CftSchemaTypeRef::Dict(_, item) = non_nullable(&current) else {
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
    schema: &CompiledSchema,
    expected: &CftSchemaTypeRef,
    value: &CfdValue,
    code: &'static str,
    stage: &'static str,
) -> Result<(), DiagnosticSet> {
    validate_value_semantics(session, schema, expected, value, &[], None, code, stage)
}

pub(crate) fn validate_value_for_write_with_pending(
    session: &ProjectSession,
    schema: &CompiledSchema,
    expected: &CftSchemaTypeRef,
    value: &CfdValue,
    pending_records: &[crate::RecordCoordinate],
    code: &'static str,
    stage: &'static str,
) -> Result<(), DiagnosticSet> {
    validate_value_semantics(
        session,
        schema,
        expected,
        value,
        pending_records,
        None,
        code,
        stage,
    )
}

pub(crate) fn validate_value_for_insert(
    session: &ProjectSession,
    schema: &CompiledSchema,
    inserted_actual_type: &str,
    inserted_key: &str,
    expected: &CftSchemaTypeRef,
    value: &CfdValue,
    pending_records: &[crate::RecordCoordinate],
    code: &'static str,
    stage: &'static str,
) -> Result<(), DiagnosticSet> {
    validate_value_semantics(
        session,
        schema,
        expected,
        value,
        pending_records,
        Some(PendingInsertRef {
            actual_type: inserted_actual_type,
            key: inserted_key,
        }),
        code,
        stage,
    )
}

fn validate_value_semantics(
    session: &ProjectSession,
    schema: &CompiledSchema,
    expected: &CftSchemaTypeRef,
    value: &CfdValue,
    pending_records: &[crate::RecordCoordinate],
    pending_insert: Option<PendingInsertRef<'_>>,
    code: &'static str,
    stage: &'static str,
) -> Result<(), DiagnosticSet> {
    let context = ProjectValueSemanticContext {
        session,
        pending_records,
    };
    coflow_data_model::validate_complete_value_for_schema(
        schema,
        &context,
        expected,
        value,
        pending_insert,
    )
    .map_err(|err| one_error(code, stage, err.message()))
}

pub(crate) fn ensure_object_type_assignable(
    schema: &CompiledSchema,
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
    pending_records: &'a [crate::RecordCoordinate],
}

impl CfdValueSemanticContext for ProjectValueSemanticContext<'_> {
    fn type_domain_id(&self, type_name: &str) -> Option<CfdDomainId> {
        self.session.model.type_domain_id(type_name)
    }

    fn record_by_domain_key(&self, domain_id: CfdDomainId, key: &str) -> Option<CfdRecordId> {
        self.session.model.record_by_domain_key(domain_id, key)
    }

    fn record_actual_type(&self, id: CfdRecordId) -> Option<&str> {
        self.session
            .model
            .record(id)
            .map(coflow_data_model::CfdRecord::actual_type)
    }

    fn pending_record_actual_type(&self, domain_id: CfdDomainId, key: &str) -> Option<&str> {
        self.pending_records
            .iter()
            .find(|record| {
                record.key == key
                    && self.session.model.type_domain_id(&record.actual_type) == Some(domain_id)
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

fn non_nullable(ty: &CftSchemaTypeRef) -> &CftSchemaTypeRef {
    match ty {
        CftSchemaTypeRef::Nullable(inner) => non_nullable(inner),
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
