use std::collections::{BTreeMap, BTreeSet};

use coflow_api::DiagnosticSet;
use coflow_cft::{CftSchemaTypeRef, CftSchemaView};
use coflow_data_model::{CfdPath, CfdPathSegment, CfdValue};

use crate::write_rules;
use crate::{ProjectSession, RecordCoordinate};

use super::coercion::{coerce_cfd_field_value, coerce_json_field_value, coerce_mutation_value};
use super::defaults::{
    create_record_draft_for_type, default_missing_fields_for_type, default_record_for_type,
    default_value_for_type_ref,
};
use super::types::{PreparedMutation, PreparedMutationOp};
use super::{one_mutation_error, one_path_error, schema_field};
use super::{
    CreateRecordDraft, DefaultMaterialization, MutationFields, MutationOp, MutationRequest,
};

pub(super) fn prepare_mutation_request(request: MutationRequest) -> PreparedMutation {
    let MutationRequest {
        check_after_write: _,
        stop_on_write_error,
        ops,
    } = request;
    let prepared_ops = ops
        .into_iter()
        .map(|op| PreparedMutationOp::Pending { op })
        .collect();
    PreparedMutation {
        stop_on_write_error,
        ops: prepared_ops,
    }
}

impl ProjectSession {
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

    /// Build a field-by-field draft for creating a new top-level record.
    ///
    /// The returned draft distinguishes schema defaults (shown in the editor
    /// but not persisted unless changed), type seeds (safe values that keep a
    /// new record loadable), and required inputs (values the host must collect
    /// from the user before calling insert).
    ///
    /// # Errors
    ///
    /// Returns diagnostics when `type_name` is unknown or cannot be inserted
    /// as a concrete top-level record draft.
    pub fn create_record_draft(
        &self,
        type_name: &str,
    ) -> Result<CreateRecordDraft, DiagnosticSet> {
        create_record_draft_for_type(&self.schema, type_name)
    }

    /// Build a default value for an item of the collection at `path`.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when the path is not a collection field or when a
    /// default reference item cannot be chosen because no target record exists.
    pub fn default_collection_item_value(
        &self,
        actual_type: &str,
        path: &[CfdPathSegment],
    ) -> Result<CfdValue, DiagnosticSet> {
        let ty = write_rules::expected_type_for_cfd_path(
            &self.schema,
            actual_type,
            path,
            "MUTATION-PATH",
            "MUTATION",
        )?;
        let item_ty = match ty.non_nullable() {
            CftSchemaTypeRef::Array(item) | CftSchemaTypeRef::Dict(_, item) => item.as_ref(),
            _ => {
                return Err(one_path_error(
                    "mutation path does not point to a collection",
                ));
            }
        };
        match item_ty.non_nullable() {
            CftSchemaTypeRef::Ref(target_type) => self
                .ref_targets(target_type)
                .into_iter()
                .next()
                .map(|target| CfdValue::Ref(target.coordinate.key))
                .ok_or_else(|| {
                    one_mutation_error(
                        "MUTATION-DEFAULT",
                        format!(
                            "collection item type `&{target_type}` has no available target record"
                        ),
                    )
                }),
            _ => default_value_for_type_ref(
                &self.schema,
                item_ty,
                DefaultMaterialization::EditableShape,
            ),
        }
    }
}

pub(super) fn prepare_one(
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
            ensure_not_dimension_storage_type(session, &actual_type, "insert")?;
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
            ensure_not_dimension_storage_type(session, &record.actual_type, "delete")?;
            ensure_file_guard(session, &record, file.as_deref())?;
            let report_file = file.or_else(|| record_file(session, &record).map(ToOwned::to_owned));
            Ok(PreparedMutationOp::DeleteRecord {
                record,
                report_file,
            })
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
                out.insert(
                    name,
                    coerce_json_field_value(session, &field.ty_ref, &value)?,
                );
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
    let Some(schema_type) = schema.type_meta(actual_type) else {
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

fn ensure_not_dimension_storage_type(
    session: &ProjectSession,
    actual_type: &str,
    operation: &str,
) -> Result<(), DiagnosticSet> {
    if session.dimension_synthesized_types().contains(actual_type) {
        return Err(one_mutation_error(
            "MUTATION-DIMENSION",
            format!(
                "dimension variant type `{actual_type}` cannot {operation} records; edit existing variant fields instead"
            ),
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
    write_rules::ensure_record_key_available_with_conflict_code(
        session,
        actual_type,
        key,
        current_record,
        "MUTATION-INSERT",
        "MUTATION-INSERT-CONFLICT",
        "MUTATION",
    )
}

fn record_file<'a>(session: &'a ProjectSession, coordinate: &RecordCoordinate) -> Option<&'a str> {
    session.file_for_record(&coordinate.actual_type, &coordinate.key)
}
