use std::collections::{BTreeMap, BTreeSet};

use coflow_api::{Diagnostic, DiagnosticSet, FlatDiagnostic, ProviderRegistry, Severity};
use coflow_cft::{CftFieldMeta, CftSchemaTypeRef, CftSchemaView};
use coflow_data_model::{CfdEnumValue, CfdPath, CfdPathSegment, CfdValue};

use crate::write_rules;
use crate::{ProjectSession, RecordCoordinate};

mod coercion;
mod defaults;
mod types;

use coercion::{coerce_cfd_field_value, coerce_json_field_value, coerce_mutation_value};
use defaults::{default_missing_fields_for_type, default_record_for_type};
pub use types::{
    DefaultMaterialization, MutationAppliedOp, MutationFailedOp, MutationFields, MutationOp,
    MutationReport, MutationRequest, MutationValue, PreparedMutation,
};
use types::PreparedMutationOp;

impl ProjectSession {
    /// Prepare a mutation request for later execution.
    ///
    /// # Errors
    ///
    /// This function currently reserves `Err` for future whole-request
    /// validation failures. Individual operations stay pending until apply
    /// time so each op can be validated against the latest session state
    /// after earlier ops in the same batch have run.
    pub fn prepare_mutation(
        &self,
        request: MutationRequest,
    ) -> Result<PreparedMutation, DiagnosticSet> {
        let MutationRequest {
            check_after_write,
            stop_on_write_error,
            ops,
        } = request;
        let prepared_ops = ops
            .into_iter()
            .map(|op| PreparedMutationOp::Pending { op })
            .collect();
        Ok(PreparedMutation {
            check_after_write,
            stop_on_write_error,
            ops: prepared_ops,
        })
    }

    /// Execute a prepared mutation request through provider writers.
    ///
    /// # Errors
    ///
    /// Returns diagnostics only when execution cannot produce a report.
    /// Per-operation validation and writer failures are represented in the
    /// returned [`MutationReport`].
    pub fn apply_prepared_mutation(
        &mut self,
        registry: &ProviderRegistry,
        prepared: PreparedMutation,
    ) -> Result<MutationReport, DiagnosticSet> {
        let PreparedMutation {
            check_after_write,
            stop_on_write_error,
            ops,
        } = prepared;
        let mut applied = Vec::new();
        let mut failed = Vec::new();
        let mut failure_diagnostics = Vec::new();
        let mut write_ok = true;

        for (index, op) in ops.iter().enumerate() {
            match apply_prepared_one(self, registry, op) {
                Ok(applied_op) => applied.push(MutationAppliedOp {
                    index,
                    ..applied_op
                }),
                Err(err) => {
                    write_ok = false;
                    let diagnostics = err.diagnostics();
                    let flat = flat_diagnostics(diagnostics);
                    failed.push(MutationFailedOp {
                        index,
                        op: prepared_op_name(op),
                        diagnostics: flat.clone(),
                    });
                    failure_diagnostics.extend(flat);
                    if stop_on_write_error || err.is_terminal() {
                        failure_diagnostics.extend(session_flat_diagnostics(self));
                        return Ok(MutationReport {
                            write_ok: false,
                            check_ok: false,
                            applied,
                            failed,
                            diagnostics: failure_diagnostics,
                        });
                    }
                }
            }
        }

        let mut diagnostics = failure_diagnostics;
        diagnostics.extend(session_flat_diagnostics(self));
        let check_ok = write_ok
            && (!check_after_write
                || diagnostics
                    .iter()
                    .all(|diagnostic| diagnostic.severity != "error"));
        Ok(MutationReport {
            write_ok,
            check_ok,
            applied,
            failed,
            diagnostics,
        })
    }

    /// Prepare and execute a mutation request.
    ///
    /// # Errors
    ///
    /// Returns diagnostics only when mutation execution cannot produce a
    /// report. Per-operation validation and writer failures are represented in
    /// the returned [`MutationReport`].
    pub fn apply_mutation(
        &mut self,
        registry: &ProviderRegistry,
        request: MutationRequest,
    ) -> Result<MutationReport, DiagnosticSet> {
        let prepared = self.prepare_mutation(request)?;
        self.apply_prepared_mutation(registry, prepared)
    }

    /// Build a schema-shaped default record value.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when `type_name` is not known in the compiled
    /// schema.
    pub fn default_record_value(
        &self,
        type_name: &str,
        materialization: DefaultMaterialization,
    ) -> Result<CfdValue, DiagnosticSet> {
        let record = default_record_for_type(&self.schema, type_name, materialization)?;
        Ok(CfdValue::Object(Box::new(record.object)))
    }
}

fn prepare_one(
    session: &ProjectSession,
    op: MutationOp,
) -> Result<PreparedMutationOp, DiagnosticSet> {
    match op {
        MutationOp::InsertRecord {
            file,
            sheet,
            actual_type,
            key,
            fields,
            materialization,
        } => {
            ensure_source_file(session, &file)?;
            ensure_type_can_insert(session, &actual_type)?;
            ensure_record_key_can_insert(session, &actual_type, &key, None)?;
            let fields = prepare_insert_fields(session, &actual_type, fields, materialization)?;
            Ok(PreparedMutationOp::InsertRecord {
                file,
                sheet,
                actual_type,
                key,
                fields,
            })
        }
        MutationOp::SetField {
            record,
            file,
            path,
            value,
        } => {
            let expected = expected_value_for_path(session, &record, &path)?;
            let (write_file, write_path) =
                effective_write_target_for_set_field(session, &record, &path)?;
            ensure_file_guard_for_file(&record, &write_file, file.as_deref())?;
            let path = cfd_path_to_write_path(&write_path)?;
            let value = coerce_mutation_value(session, &expected.ty, value)?;
            Ok(PreparedMutationOp::SetField {
                record,
                write_file,
                path,
                value,
            })
        }
        MutationOp::RenameRecord {
            record,
            file,
            new_key,
        } => {
            ensure_file_guard(session, &record, file.as_deref())?;
            let report_file = file.or_else(|| record_file(session, &record).map(ToOwned::to_owned));
            Ok(PreparedMutationOp::RenameRecord {
                record,
                new_key,
                report_file,
            })
        }
        MutationOp::DeleteRecord { record, file } => {
            ensure_file_guard(session, &record, file.as_deref())?;
            let report_file = file.or_else(|| record_file(session, &record).map(ToOwned::to_owned));
            Ok(PreparedMutationOp::DeleteRecord {
                record,
                report_file,
            })
        }
    }
}

fn apply_prepared_one(
    session: &mut ProjectSession,
    registry: &ProviderRegistry,
    op: &PreparedMutationOp,
) -> Result<MutationAppliedOp, MutationApplyError> {
    match op {
        PreparedMutationOp::Pending { op } => {
            let prepared = prepare_one(session, op.clone())
                .map_err(|diagnostics| classify_prepare_error(op, diagnostics))?;
            apply_prepared_one(session, registry, &prepared)
        }
        PreparedMutationOp::InsertRecord {
            file,
            sheet,
            actual_type,
            key,
            fields,
        } => {
            let outcome = session
                .insert_record(registry, file, sheet.as_deref(), key, actual_type, fields)
                .map_err(MutationApplyError::Terminal)?;
            Ok(MutationAppliedOp {
                index: 0,
                op: "insert_record".to_string(),
                record: Some(RecordCoordinate::new(actual_type, key)),
                file: Some(file.clone()),
                outcome,
            })
        }
        PreparedMutationOp::SetField {
            record,
            write_file,
            path,
            value,
            ..
        } => {
            let outcome = session
                .write_field(registry, &record.actual_type, &record.key, path, value)
                .map_err(MutationApplyError::Terminal)?;
            Ok(MutationAppliedOp {
                index: 0,
                op: "set_field".to_string(),
                record: Some(record.clone()),
                file: Some(write_file.clone()),
                outcome,
            })
        }
        PreparedMutationOp::RenameRecord {
            record,
            new_key,
            report_file,
        } => {
            let outcome = session
                .rename_record_key(registry, &record.actual_type, &record.key, new_key)
                .map_err(MutationApplyError::Terminal)?;
            let record = outcome.renamed.as_ref().map_or_else(
                || RecordCoordinate::new(&record.actual_type, new_key),
                |(_, new)| new.clone(),
            );
            Ok(MutationAppliedOp {
                index: 0,
                op: "rename_record".to_string(),
                record: Some(record),
                file: report_file.clone(),
                outcome,
            })
        }
        PreparedMutationOp::DeleteRecord {
            record,
            report_file,
            ..
        } => {
            let outcome = session
                .delete_record(registry, &record.actual_type, &record.key)
                .map_err(MutationApplyError::Terminal)?;
            Ok(MutationAppliedOp {
                index: 0,
                op: "delete_record".to_string(),
                record: Some(record.clone()),
                file: report_file.clone(),
                outcome,
            })
        }
    }
}

#[derive(Debug)]
enum MutationApplyError {
    Recoverable(DiagnosticSet),
    Terminal(DiagnosticSet),
}

impl MutationApplyError {
    const fn is_terminal(&self) -> bool {
        matches!(self, Self::Terminal(_))
    }

    const fn diagnostics(&self) -> &DiagnosticSet {
        match self {
            Self::Recoverable(diagnostics) | Self::Terminal(diagnostics) => diagnostics,
        }
    }
}

fn prepare_insert_fields(
    session: &ProjectSession,
    actual_type: &str,
    fields: MutationFields,
    materialization: DefaultMaterialization,
) -> Result<BTreeMap<String, CfdValue>, DiagnosticSet> {
    let provided = prepare_provided_insert_fields(session, actual_type, fields)?;
    let provided_names = provided.keys().cloned().collect::<BTreeSet<_>>();
    let mut out = default_missing_fields_for_type(
        &session.schema,
        actual_type,
        materialization,
        &provided_names,
    )?;
    out.extend(provided);
    Ok(out)
}

fn prepare_provided_insert_fields(
    session: &ProjectSession,
    actual_type: &str,
    fields: MutationFields,
) -> Result<BTreeMap<String, CfdValue>, DiagnosticSet> {
    let mut out = BTreeMap::new();
    let schema = CftSchemaView::new(&session.schema);
    match fields {
        MutationFields::Empty => {}
        MutationFields::Json(fields) => {
            for (name, value) in fields {
                let field = schema_field(&schema, actual_type, &name)?;
                out.insert(name, coerce_json_field_value(session, &field.ty_ref, &value)?);
            }
        }
        MutationFields::Cfd(fields) => {
            for (name, value) in fields {
                let field = schema_field(&schema, actual_type, &name)?;
                out.insert(name, coerce_cfd_field_value(session, &field.ty_ref, value)?);
            }
        }
    }
    Ok(out)
}

#[derive(Debug, Clone)]
struct ExpectedValue {
    ty: CftSchemaTypeRef,
}

fn expected_value_for_path(
    session: &ProjectSession,
    coordinate: &RecordCoordinate,
    path: &[CfdPathSegment],
) -> Result<ExpectedValue, DiagnosticSet> {
    let current = write_rules::expected_type_for_cfd_path(
        &session.schema,
        &coordinate.actual_type,
        path,
        "MUTATION-PATH",
        "MUTATION",
    )?;
    Ok(ExpectedValue { ty: current })
}

fn cfd_path_to_write_path(
    path: &[CfdPathSegment],
) -> Result<Vec<coflow_api::WriteFieldPathSegment>, DiagnosticSet> {
    if path.is_empty() {
        return Err(one_path_error("mutation path must not be empty"));
    }
    Ok(write_rules::cfd_path_to_write_path(path))
}

fn effective_write_target_for_set_field(
    session: &ProjectSession,
    coordinate: &RecordCoordinate,
    path: &[CfdPathSegment],
) -> Result<(String, Vec<CfdPathSegment>), DiagnosticSet> {
    let record_ref = session
        .records
        .get_by_coordinate(&coordinate.actual_type, &coordinate.key)
        .ok_or_else(|| {
            one_path_error(format!(
                "record `{}.{}` was not found",
                coordinate.actual_type, coordinate.key
            ))
        })?;
    let Some(CfdPathSegment::Field(top_field)) = path.first() else {
        return Ok((record_ref.display_path.clone(), path.to_vec()));
    };
    let _record = session.model.record(record_ref.id).ok_or_else(|| {
        one_path_error(format!(
            "record `{}.{}` was not found in the data model",
            coordinate.actual_type, coordinate.key
        ))
    })?;
    let cfd_path = CfdPath {
        segments: path.to_vec(),
    };
    let Some((source_id, source_path)) = session.model.spread_source_path(record_ref.id, &cfd_path)
    else {
        return Ok((record_ref.display_path.clone(), path.to_vec()));
    };
    session
        .records
        .get(source_id)
        .map(|source_ref| {
            (
                source_ref.display_path.clone(),
                source_path.segments.clone(),
            )
        })
        .ok_or_else(|| {
            one_path_error(format!(
                "spread source for field `{top_field}` is no longer indexed"
            ))
        })
}

pub(super) fn schema_field<'a>(
    schema: &'a CftSchemaView,
    actual_type: &str,
    field_name: &str,
) -> Result<&'a CftFieldMeta, DiagnosticSet> {
    let Some(schema_type) = schema.types.get(actual_type) else {
        return Err(one_mutation_error(
            "MUTATION-TYPE",
            format!("unknown type `{actual_type}`"),
        ));
    };
    schema_type
        .all_fields
        .iter()
        .find(|field| field.name == field_name)
        .ok_or_else(|| {
            one_path_error(format!(
                "unknown field `{field_name}` on type `{actual_type}`"
            ))
        })
}

fn ensure_source_file(session: &ProjectSession, file: &str) -> Result<(), DiagnosticSet> {
    if session.files.source_files().contains(file) {
        return Ok(());
    }
    Err(one_mutation_error(
        "MUTATION-FILE",
        format!("file `{file}` is not a loaded data source"),
    ))
}

fn ensure_file_guard(
    session: &ProjectSession,
    coordinate: &RecordCoordinate,
    file: Option<&str>,
) -> Result<(), DiagnosticSet> {
    let Some(expected_file) = file else {
        return Ok(());
    };
    let Some(actual_file) = record_file(session, coordinate) else {
        return Err(one_mutation_error(
            "MUTATION-FILE-GUARD",
            format!(
                "record `{}.{}` was not found for file guard `{expected_file}`",
                coordinate.actual_type, coordinate.key
            ),
        ));
    };
    if actual_file == expected_file {
        return Ok(());
    }
    Err(one_mutation_error(
        "MUTATION-FILE-GUARD",
        format!(
            "record `{}.{}` belongs to `{actual_file}`, not `{expected_file}`",
            coordinate.actual_type, coordinate.key
        ),
    ))
}

fn ensure_file_guard_for_file(
    coordinate: &RecordCoordinate,
    actual_file: &str,
    expected_file: Option<&str>,
) -> Result<(), DiagnosticSet> {
    let Some(expected_file) = expected_file else {
        return Ok(());
    };
    if actual_file == expected_file {
        return Ok(());
    }
    Err(one_mutation_error(
        "MUTATION-FILE-GUARD",
        format!(
            "record `{}.{}` writes to `{actual_file}`, not `{expected_file}`",
            coordinate.actual_type, coordinate.key
        ),
    ))
}

fn ensure_type_can_insert(
    session: &ProjectSession,
    actual_type: &str,
) -> Result<(), DiagnosticSet> {
    let schema = CftSchemaView::new(&session.schema);
    let Some(schema_type) = schema.types.get(actual_type) else {
        return Err(one_mutation_error(
            "MUTATION-TYPE",
            format!("unknown insert type `{actual_type}`"),
        ));
    };
    if schema_type.is_abstract {
        return Err(one_mutation_error(
            "MUTATION-TYPE",
            format!("abstract type `{actual_type}` cannot be inserted"),
        ));
    }
    if schema_type.is_singleton {
        return Err(one_mutation_error(
            "MUTATION-TYPE",
            format!("singleton type `{actual_type}` cannot be inserted"),
        ));
    }
    Ok(())
}

fn ensure_record_key_can_insert(
    session: &ProjectSession,
    actual_type: &str,
    key: &str,
    current_record: Option<coflow_data_model::CfdRecordId>,
) -> Result<(), DiagnosticSet> {
    write_rules::ensure_record_key_available(
        session,
        actual_type,
        key,
        current_record,
        "MUTATION-INSERT",
        "MUTATION",
    )
}

pub(super) fn enum_value(
    session: &ProjectSession,
    enum_name: &str,
    raw_variant: &str,
) -> Result<CfdEnumValue, DiagnosticSet> {
    let variant = raw_variant
        .strip_prefix(enum_name)
        .and_then(|rest| rest.strip_prefix('.'))
        .unwrap_or(raw_variant);
    let schema = CftSchemaView::new(&session.schema);
    let int_value = schema
        .enum_variant_value(enum_name, variant)
        .ok_or_else(|| one_value_error(format!("unknown enum variant `{enum_name}.{variant}`")))?;
    Ok(CfdEnumValue {
        enum_name: enum_name.to_string(),
        variant: Some(variant.to_string()),
        value: int_value,
    })
}

pub(super) fn is_schema_enum(session: &ProjectSession, name: &str) -> bool {
    CftSchemaView::new(&session.schema).enums.contains_key(name)
}

fn non_nullable(ty: &CftSchemaTypeRef) -> &CftSchemaTypeRef {
    match ty {
        CftSchemaTypeRef::Nullable(inner) => non_nullable(inner),
        other => other,
    }
}

fn record_file<'a>(session: &'a ProjectSession, coordinate: &RecordCoordinate) -> Option<&'a str> {
    session.file_for_record(&coordinate.actual_type, &coordinate.key)
}

fn prepared_op_name(op: &PreparedMutationOp) -> String {
    match op {
        PreparedMutationOp::Pending { op } => mutation_op_name(op).to_string(),
        PreparedMutationOp::InsertRecord { .. } => "insert_record".to_string(),
        PreparedMutationOp::SetField { .. } => "set_field".to_string(),
        PreparedMutationOp::RenameRecord { .. } => "rename_record".to_string(),
        PreparedMutationOp::DeleteRecord { .. } => "delete_record".to_string(),
    }
}

const fn mutation_op_name(op: &MutationOp) -> &'static str {
    match op {
        MutationOp::InsertRecord { .. } => "insert_record",
        MutationOp::SetField { .. } => "set_field",
        MutationOp::RenameRecord { .. } => "rename_record",
        MutationOp::DeleteRecord { .. } => "delete_record",
    }
}

fn classify_prepare_error(op: &MutationOp, diagnostics: DiagnosticSet) -> MutationApplyError {
    let terminal_insert_conflict = matches!(op, MutationOp::InsertRecord { .. })
        && diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "MUTATION-INSERT" && diagnostic.message.contains("already exists")
        });
    if terminal_insert_conflict {
        MutationApplyError::Terminal(diagnostics)
    } else {
        MutationApplyError::Recoverable(diagnostics)
    }
}

fn one_path_error(message: impl Into<String>) -> DiagnosticSet {
    one_mutation_error("MUTATION-PATH", message)
}

pub(super) fn one_value_error(message: impl Into<String>) -> DiagnosticSet {
    one_mutation_error("MUTATION-VALUE", message)
}

fn one_mutation_error(code: &'static str, message: impl Into<String>) -> DiagnosticSet {
    DiagnosticSet::one(Diagnostic {
        code: code.to_string(),
        stage: "MUTATION".to_string(),
        severity: Severity::Error,
        message: message.into(),
        primary: None,
        related: Vec::new(),
    })
}

fn session_flat_diagnostics(session: &ProjectSession) -> Vec<FlatDiagnostic> {
    session
        .diagnostics
        .as_set()
        .diagnostics
        .iter()
        .enumerate()
        .map(|(index, diagnostic)| {
            let location = session.diagnostics.logical_location(index);
            let actual_type = location.and_then(|l| l.actual_type.clone());
            let record_key = location.and_then(|l| l.record_key.clone());
            let field_path = location.and_then(|l| l.field_path.clone());
            diagnostic.flat_view(actual_type, record_key, field_path)
        })
        .collect()
}

fn flat_diagnostics(diagnostics: &DiagnosticSet) -> Vec<FlatDiagnostic> {
    diagnostics
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.flat_view(None, None, None))
        .collect()
}
