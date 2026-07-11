use std::collections::{BTreeMap, BTreeSet};

use coflow_api::DiagnosticSet;
use coflow_api::WriteFieldPathSegment;
use coflow_cft::CftSchemaTypeRef;
use coflow_data_model::{CfdPathSegment, CfdValue};

use crate::write_rules;
use crate::writes;
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
        let record = default_record_for_type(self.compiled_schema(), type_name, materialization)?;
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
    pub fn create_record_draft(&self, type_name: &str) -> Result<CreateRecordDraft, DiagnosticSet> {
        create_record_draft_for_type(self.compiled_schema(), type_name)
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
                self.compiled_schema(),
                item_ty,
                DefaultMaterialization::EditableShape,
            ),
        }
    }
}

pub(super) fn prepare_one(
    session: &ProjectSession,
    op: MutationOp,
    pending_records: &[RecordCoordinate],
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
            ensure_record_key_available(
                session,
                &actual_type,
                &key,
                None,
                "MUTATION-INSERT",
                "MUTATION-INSERT-CONFLICT",
            )?;
            let fields = prepare_insert_fields(
                session,
                &actual_type,
                &key,
                fields,
                materialization,
                pending_records,
            )?;
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
            let path = validated_write_path(&path)?;
            let (write_record, write_file, path) =
                effective_write_target_for_set_field(session, &record, &path)?;
            ensure_file_guard_for_file(&record, &write_file, file.as_deref())?;
            let value = coerce_mutation_value(session, &expected.ty, value, pending_records)?;
            Ok(PreparedMutationOp::SetField {
                record,
                write_record,
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
            let current_record = session
                .records
                .id_for_coordinate(&record.actual_type, &record.key)
                .ok_or_else(|| {
                    one_mutation_error(
                        "MUTATION-RENAME",
                        format!(
                            "record `{}.{}` was not found",
                            record.actual_type, record.key
                        ),
                    )
                })?;
            ensure_record_key_available(
                session,
                &record.actual_type,
                &new_key,
                Some(current_record),
                "MUTATION-RENAME",
                "MUTATION-RENAME-CONFLICT",
            )?;
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

pub(super) fn prepare_set_on_pending_insert(
    session: &ProjectSession,
    insert_file: &str,
    actual_type: &str,
    key: &str,
    fields: &mut BTreeMap<String, CfdValue>,
    file_guard: Option<&str>,
    path: &[CfdPathSegment],
    value: super::MutationValue,
    pending_records: &[RecordCoordinate],
) -> Result<PreparedMutationOp, DiagnosticSet> {
    ensure_file_guard_for_file(
        &RecordCoordinate::new(actual_type, key),
        insert_file,
        file_guard,
    )?;
    let expected = write_rules::expected_type_for_cfd_path(
        &session.schema,
        actual_type,
        path,
        "MUTATION-PATH",
        "MUTATION",
    )?;
    let path = validated_write_path(path)?;
    let value = coerce_mutation_value(session, &expected, value, pending_records)?;
    set_pending_insert_value(fields, &path, value)?;
    Ok(PreparedMutationOp::FoldedSetField {
        record: RecordCoordinate::new(actual_type, key),
        write_file: insert_file.to_string(),
    })
}

pub(super) fn prepare_rename_on_pending_insert(
    session: &ProjectSession,
    insert_file: &str,
    record: &RecordCoordinate,
    file_guard: Option<&str>,
    new_key: &str,
) -> Result<PreparedMutationOp, DiagnosticSet> {
    ensure_file_guard_for_file(record, insert_file, file_guard)?;
    ensure_record_key_available(
        session,
        &record.actual_type,
        new_key,
        None,
        "MUTATION-RENAME",
        "MUTATION-RENAME-CONFLICT",
    )?;
    Ok(PreparedMutationOp::FoldedRenameRecord {
        old_record: record.clone(),
        new_record: RecordCoordinate::new(&record.actual_type, new_key),
        write_file: insert_file.to_string(),
    })
}

pub(super) fn prepare_delete_on_pending_insert(
    insert_file: &str,
    record: &RecordCoordinate,
    file_guard: Option<&str>,
) -> Result<PreparedMutationOp, DiagnosticSet> {
    ensure_file_guard_for_file(record, insert_file, file_guard)?;
    Ok(PreparedMutationOp::FoldedDeleteRecord {
        record: record.clone(),
        write_file: insert_file.to_string(),
    })
}

pub(super) fn rename_pending_insert_references(
    session: &ProjectSession,
    target_actual_type: &str,
    host_actual_type: &str,
    fields: &mut BTreeMap<String, CfdValue>,
    old_key: &str,
    new_key: &str,
) -> Result<(), DiagnosticSet> {
    let schema = session.compiled_schema();
    for (name, value) in fields {
        let field = schema_field(&schema, host_actual_type, name)?;
        rename_pending_value_references(
            &schema,
            target_actual_type,
            &field.ty_ref,
            value,
            old_key,
            new_key,
        );
    }
    Ok(())
}

pub(super) fn rename_prepared_field_references(
    session: &ProjectSession,
    target_actual_type: &str,
    host_actual_type: &str,
    path: &[WriteFieldPathSegment],
    value: &mut CfdValue,
    old_key: &str,
    new_key: &str,
) -> Result<(), DiagnosticSet> {
    let schema = session.compiled_schema();
    let expected = write_rules::expected_type_for_cfd_path_in_view(
        &schema,
        host_actual_type,
        path,
        "MUTATION-PATH",
        "MUTATION",
    )?;
    rename_pending_value_references(
        &schema,
        target_actual_type,
        &expected,
        value,
        old_key,
        new_key,
    );
    Ok(())
}

fn rename_pending_value_references(
    schema: &coflow_cft::CompiledSchema,
    target_actual_type: &str,
    expected: &CftSchemaTypeRef,
    value: &mut CfdValue,
    old_key: &str,
    new_key: &str,
) {
    match (expected, value) {
        (CftSchemaTypeRef::Nullable(inner), value) => rename_pending_value_references(
            schema,
            target_actual_type,
            inner,
            value,
            old_key,
            new_key,
        ),
        (CftSchemaTypeRef::Ref(target_type), CfdValue::Ref(key))
            if key == old_key && schema.is_assignable(target_actual_type, target_type) =>
        {
            *key = new_key.to_string();
        }
        (CftSchemaTypeRef::Array(inner), CfdValue::Array(items)) => {
            for item in items {
                rename_pending_value_references(
                    schema,
                    target_actual_type,
                    inner,
                    item,
                    old_key,
                    new_key,
                );
            }
        }
        (CftSchemaTypeRef::Dict(_, item_type), CfdValue::Dict(entries)) => {
            for (_, item) in entries {
                rename_pending_value_references(
                    schema,
                    target_actual_type,
                    item_type,
                    item,
                    old_key,
                    new_key,
                );
            }
        }
        (CftSchemaTypeRef::Named(_), CfdValue::Object(object)) => {
            let actual_type = object.actual_type().to_string();
            for (name, field_value) in object.fields_mut() {
                let Some(field_type) = schema.field_type(&actual_type, name) else {
                    continue;
                };
                rename_pending_value_references(
                    schema,
                    target_actual_type,
                    field_type,
                    field_value,
                    old_key,
                    new_key,
                );
            }
        }
        _ => {}
    }
}

fn set_pending_insert_value(
    fields: &mut BTreeMap<String, CfdValue>,
    path: &[CfdPathSegment],
    value: CfdValue,
) -> Result<(), DiagnosticSet> {
    let Some(CfdPathSegment::Field(field)) = path.first() else {
        return Err(one_path_error(
            "pending insert paths must start with a field name",
        ));
    };
    if path.len() == 1 {
        fields.insert(field.clone(), value);
        return Ok(());
    }
    let Some(current) = fields.get_mut(field) else {
        return Err(one_path_error(format!(
            "field `{field}` has no materialized value for a nested pending-insert write"
        )));
    };
    set_nested_value(current, &path[1..], value)
}

fn set_nested_value(
    current: &mut CfdValue,
    path: &[CfdPathSegment],
    value: CfdValue,
) -> Result<(), DiagnosticSet> {
    let Some((segment, rest)) = path.split_first() else {
        *current = value;
        return Ok(());
    };
    let next = match (current, segment) {
        (CfdValue::Object(object), CfdPathSegment::Field(field)) => object.fields.get_mut(field),
        (CfdValue::Array(items), CfdPathSegment::Index(index)) => items.get_mut(*index),
        (CfdValue::Dict(entries), CfdPathSegment::DictKey(key)) => entries
            .iter_mut()
            .find(|(entry_key, _)| crate::dict_key_path_text(entry_key) == *key)
            .map(|(_, entry_value)| entry_value),
        _ => None,
    }
    .ok_or_else(|| one_path_error("pending insert nested path was not materialized"))?;
    set_nested_value(next, rest, value)
}

fn prepare_insert_fields(
    session: &ProjectSession,
    actual_type: &str,
    key: &str,
    fields: MutationFields,
    materialization: DefaultMaterialization,
    pending_records: &[RecordCoordinate],
) -> Result<BTreeMap<String, CfdValue>, DiagnosticSet> {
    let provided = prepare_provided_insert_fields(session, actual_type, fields)?;
    let provided_names = provided.keys().cloned().collect::<BTreeSet<_>>();
    let mut out = default_missing_fields_for_type(
        session.compiled_schema(),
        actual_type,
        materialization,
        &provided_names,
    )?;
    out.extend(provided);
    let schema = session.compiled_schema();
    for (name, value) in &out {
        let field = schema_field(&schema, actual_type, name)?;
        write_rules::validate_value_for_insert_in_view(
            session,
            &schema,
            actual_type,
            key,
            &field.ty_ref,
            value,
            pending_records,
            "MUTATION-SHAPE",
            "MUTATION",
        )?;
    }
    Ok(out)
}

fn prepare_provided_insert_fields(
    session: &ProjectSession,
    actual_type: &str,
    fields: MutationFields,
) -> Result<BTreeMap<String, CfdValue>, DiagnosticSet> {
    let mut out = BTreeMap::new();
    let schema = session.compiled_schema();
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

fn validated_write_path(
    path: &[CfdPathSegment],
) -> Result<Vec<WriteFieldPathSegment>, DiagnosticSet> {
    if path.is_empty() {
        return Err(one_path_error("mutation path must not be empty"));
    }
    Ok(path.to_vec())
}

fn effective_write_target_for_set_field(
    session: &ProjectSession,
    coordinate: &RecordCoordinate,
    path: &[WriteFieldPathSegment],
) -> Result<(RecordCoordinate, String, Vec<WriteFieldPathSegment>), DiagnosticSet> {
    let record_ref = session
        .records
        .get_by_coordinate(&coordinate.actual_type, &coordinate.key)
        .ok_or_else(|| {
            one_path_error(format!(
                "record `{}.{}` was not found",
                coordinate.actual_type, coordinate.key
            ))
        })?;
    let _record = session.model.record(record_ref.id).ok_or_else(|| {
        one_path_error(format!(
            "record `{}.{}` was not found in the data model",
            coordinate.actual_type, coordinate.key
        ))
    })?;
    writes::effective_write_target_for_path(session, record_ref, path)
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
    let schema = session.compiled_schema();
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

fn ensure_record_key_available(
    session: &ProjectSession,
    actual_type: &str,
    key: &str,
    current_record: Option<coflow_data_model::CfdRecordId>,
    code: &'static str,
    conflict_code: &'static str,
) -> Result<(), DiagnosticSet> {
    write_rules::ensure_record_key_available_with_conflict_code(
        session,
        actual_type,
        key,
        current_record,
        code,
        conflict_code,
        "MUTATION",
    )
}

fn record_file<'a>(session: &'a ProjectSession, coordinate: &RecordCoordinate) -> Option<&'a str> {
    session.file_for_record(&coordinate.actual_type, &coordinate.key)
}
