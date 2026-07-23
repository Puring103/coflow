use std::collections::{BTreeMap, BTreeSet};

use coflow_api::DiagnosticSet;
use coflow_api::WriteFieldPathSegment;
use coflow_cft::{CftValueType, RecordKey};
use coflow_data_model::{CfdPath, CfdPathSegment, CfdValue, PendingInsertRef};

use crate::write_rules;
use crate::writes;
use crate::{ProjectSession, RecordCoordinate};

use super::coercion::{coerce_cfd_field_value, coerce_json_field_value, coerce_mutation_value};
use super::defaults::{
    create_record_draft_for_type, default_missing_fields_for_type, default_object_for_type,
    default_value_for_value_type,
};
use super::types::PreparedMutationOp;
use super::{one_mutation_error, one_path_error, schema_field, validated_record_coordinate};
use super::{CreateRecordDraft, DefaultMaterialization, MutationFields, MutationOp};

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
        let object = default_object_for_type(self.schema(), type_name, materialization)?;
        Ok(CfdValue::Object(Box::new(object)))
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
        create_record_draft_for_type(self.schema(), type_name)
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
            self.schema(),
            actual_type,
            path,
            "MUTATION-PATH",
            "MUTATION",
        )?;
        let item_ty = match ty.non_nullable() {
            CftValueType::Array(item) | CftValueType::Dict(_, item) => item.as_ref(),
            _ => {
                return Err(one_path_error(
                    "mutation path does not point to a collection",
                ));
            }
        };
        match item_ty.non_nullable() {
            CftValueType::RecordRef(target_type) => self
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
            _ => default_value_for_value_type(
                self.schema(),
                item_ty,
                DefaultMaterialization::EditableShape,
            ),
        }
    }
}

#[allow(clippy::too_many_lines)]
pub(super) fn prepare_one(
    session: &ProjectSession,
    op: MutationOp,
    pending_records: &BTreeMap<RecordCoordinate, usize>,
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
            let coordinate = validated_record_coordinate(actual_type, key)?;
            Ok(PreparedMutationOp::InsertRecord {
                file,
                sheet,
                actual_type: coordinate.actual_type,
                key: coordinate.key,
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
        MutationOp::SetDimensionValue {
            coordinate,
            expected,
            value,
        } => super::dimension::prepare_dimension_value(
            session,
            coordinate,
            expected,
            Some(value),
            pending_records,
        ),
        MutationOp::ClearDimensionValue {
            coordinate,
            expected,
        } => super::dimension::prepare_dimension_value(
            session,
            coordinate,
            expected,
            None,
            pending_records,
        ),
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
            let new_key = RecordKey::new(new_key)
                .map_err(|error| one_mutation_error("MUTATION-RENAME", error.to_string()))?;
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
        MutationOp::SwapRecords {
            first,
            second,
            file,
        } => {
            ensure_file_guard(session, &first, file.as_deref())?;
            ensure_file_guard(session, &second, file.as_deref())?;
            let first_file = required_record_file(session, &first, "MUTATION-REORDER")?;
            let second_file = required_record_file(session, &second, "MUTATION-REORDER")?;
            if first_file != second_file {
                return Err(one_mutation_error(
                    "MUTATION-REORDER-CONTAINER",
                    "records must belong to the same source file",
                ));
            }
            Ok(PreparedMutationOp::SwapRecords {
                first,
                second,
                report_file: first_file.to_string(),
            })
        }
        MutationOp::MoveRecord {
            record,
            target_index,
            file,
        } => {
            ensure_file_guard(session, &record, file.as_deref())?;
            let report_file = required_record_file(session, &record, "MUTATION-REORDER")?;
            Ok(PreparedMutationOp::MoveRecord {
                record,
                target_index,
                report_file: report_file.to_string(),
            })
        }
        MutationOp::TransferRecord {
            record,
            destination_file,
            destination_sheet,
            target_index,
            source_file,
        } => {
            ensure_file_guard(session, &record, source_file.as_deref())?;
            let source_file = required_record_file(session, &record, "MUTATION-TRANSFER")?;
            if source_file == destination_file {
                return Err(one_mutation_error(
                    "MUTATION-TRANSFER-FILE",
                    "record transfer requires different source and destination files",
                ));
            }
            Ok(PreparedMutationOp::TransferRecord {
                record,
                destination_file,
                destination_sheet,
                target_index,
            })
        }
    }
}

pub(super) struct PendingInsertSetRequest<'a> {
    pub(super) insert_file: &'a str,
    pub(super) actual_type: &'a str,
    pub(super) key: &'a str,
    pub(super) fields: &'a mut BTreeMap<String, CfdValue>,
    pub(super) file_guard: Option<&'a str>,
    pub(super) path: &'a [CfdPathSegment],
    pub(super) value: super::MutationValue,
    pub(super) pending_records: &'a BTreeMap<RecordCoordinate, usize>,
}

pub(super) fn prepare_set_on_pending_insert(
    session: &ProjectSession,
    request: PendingInsertSetRequest<'_>,
) -> Result<PreparedMutationOp, DiagnosticSet> {
    let PendingInsertSetRequest {
        insert_file,
        actual_type,
        key,
        fields,
        file_guard,
        path,
        value,
        pending_records,
    } = request;
    ensure_file_guard_for_file(
        &validated_record_coordinate(actual_type, key)?,
        insert_file,
        file_guard,
    )?;
    let expected = write_rules::expected_type_for_cfd_path(
        session.schema(),
        actual_type,
        path,
        "MUTATION-PATH",
        "MUTATION",
    )?;
    let path = validated_write_path(path)?;
    let value = coerce_mutation_value(session, &expected, value, pending_records)?;
    set_pending_insert_value(fields, &path, value)?;
    Ok(PreparedMutationOp::FoldedSetField {
        record: validated_record_coordinate(actual_type, key)?,
        write_file: insert_file.to_string(),
        path: CfdPath {
            segments: path.to_vec(),
        },
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
    let new_key = RecordKey::new(new_key)
        .map_err(|error| one_mutation_error("MUTATION-RENAME", error.to_string()))?;
    Ok(PreparedMutationOp::FoldedRenameRecord {
        old_record: record.clone(),
        new_record: RecordCoordinate::new(record.actual_type.clone(), new_key),
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
    new_key: &RecordKey,
) -> Result<(), DiagnosticSet> {
    let schema = session.schema();
    for (name, value) in fields {
        let field = schema_field(schema, host_actual_type, name)?;
        rename_pending_value_references(
            schema,
            target_actual_type,
            &field.value_type,
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
    new_key: &RecordKey,
) -> Result<(), DiagnosticSet> {
    let schema = session.schema();
    let expected = write_rules::expected_type_for_cfd_path(
        schema,
        host_actual_type,
        path,
        "MUTATION-PATH",
        "MUTATION",
    )?;
    rename_pending_value_references(
        schema,
        target_actual_type,
        &expected,
        value,
        old_key,
        new_key,
    );
    Ok(())
}

fn rename_pending_value_references(
    schema: &coflow_cft::CftSchema,
    target_actual_type: &str,
    expected: &CftValueType,
    value: &mut CfdValue,
    old_key: &str,
    new_key: &RecordKey,
) {
    match (expected, value) {
        (CftValueType::Nullable(inner), value) => rename_pending_value_references(
            schema,
            target_actual_type,
            inner,
            value,
            old_key,
            new_key,
        ),
        (CftValueType::RecordRef(target_type), CfdValue::Ref(key))
            if key.as_str() == old_key && schema.is_assignable(target_actual_type, target_type) =>
        {
            *key = new_key.clone();
        }
        (CftValueType::Array(inner), CfdValue::Array(items)) => {
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
        (CftValueType::Dict(_, item_type), CfdValue::Dict(entries)) => {
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
        (CftValueType::Object(_), CfdValue::Object(object)) => {
            let actual_type = object.actual_type().to_string();
            for (name, field_value) in object.fields_mut() {
                let Some(field) = schema.field(&actual_type, name.as_str()) else {
                    continue;
                };
                rename_pending_value_references(
                    schema,
                    target_actual_type,
                    &field.value_type,
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

pub(super) fn set_nested_value(
    current: &mut CfdValue,
    path: &[CfdPathSegment],
    value: CfdValue,
) -> Result<(), DiagnosticSet> {
    let Some((segment, rest)) = path.split_first() else {
        *current = value;
        return Ok(());
    };
    let next = match (current, segment) {
        (CfdValue::Object(object), CfdPathSegment::Field(field)) => {
            object.fields.get_mut(field.as_str())
        }
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
    pending_records: &BTreeMap<RecordCoordinate, usize>,
) -> Result<BTreeMap<String, CfdValue>, DiagnosticSet> {
    let provided = prepare_provided_insert_fields(session, actual_type, fields)?;
    let provided_names = provided.keys().cloned().collect::<BTreeSet<_>>();
    let mut out = default_missing_fields_for_type(
        session.schema(),
        actual_type,
        materialization,
        &provided_names,
    )?;
    out.extend(provided);
    let schema = session.schema();
    for (name, value) in &out {
        let field = schema_field(schema, actual_type, name)?;
        write_rules::validate_value_semantics(
            session,
            schema,
            coflow_data_model::ValueValidationRequest::new(
                &field.value_type,
                value,
                coflow_data_model::ValueValidationMode::Mutation,
            )
            .with_pending_insert(PendingInsertRef { actual_type, key }),
            Some(pending_records),
            "MUTATION-VALUE",
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
    let schema = session.schema();
    match fields {
        MutationFields::Empty => {}
        MutationFields::Json(fields) => {
            for (name, value) in fields {
                let field = schema_field(schema, actual_type, &name)?;
                out.insert(
                    name,
                    coerce_json_field_value(session, &field.value_type, &value)?,
                );
            }
        }
        MutationFields::Cfd(fields) => {
            for (name, value) in fields {
                schema_field(schema, actual_type, &name)?;
                out.insert(name, coerce_cfd_field_value(session, value)?);
            }
        }
    }
    Ok(out)
}

#[derive(Debug, Clone)]
struct ExpectedValue {
    ty: CftValueType,
}

fn expected_value_for_path(
    session: &ProjectSession,
    coordinate: &RecordCoordinate,
    path: &[CfdPathSegment],
) -> Result<ExpectedValue, DiagnosticSet> {
    let current = write_rules::expected_type_for_cfd_path(
        session.schema(),
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
    let schema = session.schema();
    let Some(schema_type) = schema.resolve_type(actual_type) else {
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

fn required_record_file<'a>(
    session: &'a ProjectSession,
    coordinate: &RecordCoordinate,
    code: &'static str,
) -> Result<&'a str, DiagnosticSet> {
    record_file(session, coordinate).ok_or_else(|| {
        one_mutation_error(
            code,
            format!(
                "record `{}.{}` was not found",
                coordinate.actual_type, coordinate.key
            ),
        )
    })
}
