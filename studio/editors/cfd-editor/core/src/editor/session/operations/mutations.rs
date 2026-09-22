//! Mutation 命令：字段写回、集合编辑、记录增删改与排序。
//!
//! 项目层拥有校验与落盘，本层只做 MutationReport 到 wire DTO 的包装。

use super::super::errors::api_diagnostics_to_editor_error;
use super::super::{
    mutation_apply::{finalize_mutation, write_field_in_session},
    row_build::{
        record_container_index, record_type_index, reorder_file_path, snapshot_record_before_delete,
    },
    SessionStore,
};
use crate::editor::types::{
    BatchWriteFieldEditOutcome, BatchWriteFieldInput, BatchWriteFieldOutcome, CollectionEdit,
    DeleteRecordOutcome, EditorError, InsertRecordOutcome, RenameRecordOutcome,
    ReorderRecordsOutcome, WriteFieldOutcome,
};
use coflow_project::{
    CfdValue, DefaultMaterialization, MutationFields, MutationOp, MutationRequest, MutationValue,
    RecordCoordinate,
};

impl SessionStore {
    /// Persist a single field edit addressed by its owner record coordinate.
    #[allow(clippy::too_many_lines)]
    pub fn write_field(
        &self,
        id: u32,
        coordinate: &RecordCoordinate,
        field_path: &[coflow_project::CfdPathSegment],
        new_value: &CfdValue,
    ) -> Result<WriteFieldOutcome, EditorError> {
        let entry = self.session(id)?;
        let mut session = entry.state.write();
        session.ensure_writable()?;
        write_field_in_session(&mut session, coordinate, field_path, new_value)
    }

    pub fn write_fields(
        &self,
        id: u32,
        writes: &[BatchWriteFieldInput],
    ) -> Result<BatchWriteFieldOutcome, EditorError> {
        let entry = self.session(id)?;
        let mut session = entry.state.write();
        session.ensure_writable()?;
        let mut seen = std::collections::HashSet::new();
        let targets = writes
            .iter()
            .filter(|write| seen.insert((write.coordinate.clone(), write.field_path.clone())))
            .filter_map(|write| {
                let old_value = session
                    .queries()
                    .effective_field_write(&write.coordinate, &write.field_path)
                    .and_then(|preview| preview.old_value);
                (old_value.as_ref() != Some(&write.new_value)).then(|| (write.clone(), old_value))
            })
            .collect::<Vec<_>>();
        if targets.is_empty() {
            return Err(EditorError::write("batch field write contains no changes"));
        }
        let report = coflow_project::commands::apply_project_mutation(
            &mut session.project_session,
            MutationRequest {
                stop_on_write_error: true,
                ops: targets
                    .iter()
                    .map(|(write, _)| MutationOp::SetField {
                        record: write.coordinate.clone(),
                        file: None,
                        path: write.field_path.clone(),
                        value: MutationValue::Cfd(write.new_value.clone()),
                    })
                    .collect(),
            },
        )
        .map_err(api_diagnostics_to_editor_error)?;
        let (report, changes) =
            finalize_mutation(&mut session, report, "batch field write failed")?;
        let edits = targets
            .into_iter()
            .enumerate()
            .map(|(index, (write, old_value))| {
                let final_coordinate = report
                    .applied
                    .iter()
                    .find(|applied| applied.index == index)
                    .and_then(|applied| applied.outcome.renamed.as_ref())
                    .and_then(|(old, new)| (old == &write.coordinate).then(|| new.clone()))
                    .unwrap_or_else(|| write.coordinate.clone());
                let new_value = session
                    .queries()
                    .field_value(
                        &final_coordinate.actual_type,
                        &final_coordinate.key,
                        &write.field_path,
                    )
                    .cloned();
                BatchWriteFieldEditOutcome {
                    coordinate: write.coordinate,
                    final_coordinate,
                    field_path: write.field_path,
                    old_value,
                    new_value,
                }
            })
            .collect();
        Ok(BatchWriteFieldOutcome {
            changes,
            revision: session.revisions.current(),
            edits,
            diagnostics: report.diagnostics,
            affected_files: report.affected_files,
        })
    }

    pub fn edit_collection(
        &self,
        id: u32,
        coordinate: &RecordCoordinate,
        field_path: &[coflow_project::CfdPathSegment],
        edit: CollectionEdit,
    ) -> Result<WriteFieldOutcome, EditorError> {
        let entry = self.session(id)?;
        let mut session = entry.state.write();
        session.ensure_writable()?;
        let current = session
            .queries()
            .field_value(&coordinate.actual_type, &coordinate.key, field_path)
            .cloned()
            .ok_or_else(|| EditorError::not_found("collection field not found"))?;
        let default_item = session
            .project_session
            .default_collection_item_value_for_record(coordinate, field_path)
            .ok();
        let next = coflow_project::apply_collection_edit(current, edit, default_item)
            .map_err(api_diagnostics_to_editor_error)?;
        let outcome = write_field_in_session(&mut session, coordinate, field_path, &next);
        drop(session);
        outcome
    }

    pub fn insert_record(
        &self,
        id: u32,
        file_path: &str,
        record_key: &str,
        actual_type: &str,
        fields: CfdValue,
    ) -> Result<InsertRecordOutcome, EditorError> {
        self.insert_record_with_materialization(
            id,
            file_path,
            record_key,
            actual_type,
            fields,
            DefaultMaterialization::Minimal,
        )
    }

    pub fn insert_record_with_materialization(
        &self,
        id: u32,
        file_path: &str,
        record_key: &str,
        actual_type: &str,
        fields: CfdValue,
        materialization: DefaultMaterialization,
    ) -> Result<InsertRecordOutcome, EditorError> {
        let entry = self.session(id)?;
        let session_lock = &entry.state;
        let CfdValue::Object(boxed) = fields else {
            return Err(EditorError::write(
                "insert_record requires a CfdValue::Object for fields",
            ));
        };
        let fields_map = boxed
            .fields
            .into_iter()
            .map(|(name, value)| (name.to_string(), value))
            .collect();

        let mut session = session_lock.write();
        session.ensure_writable()?;
        let report = coflow_project::commands::apply_project_mutation(
            &mut session.project_session,
            MutationRequest {
                stop_on_write_error: true,
                ops: vec![MutationOp::InsertRecord {
                    file: file_path.to_string(),
                    actual_type: actual_type.to_string(),
                    key: record_key.to_string(),
                    fields: MutationFields::Cfd(fields_map),
                    materialization,
                }],
            },
        )
        .map_err(api_diagnostics_to_editor_error)?;
        let (report, changes) = finalize_mutation(&mut session, report, "insert record failed")?;
        Ok(InsertRecordOutcome {
            changes,
            revision: session.revisions.current(),
            diagnostics: report.diagnostics,
            affected_files: report.affected_files,
        })
    }

    pub fn rename_record_key(
        &self,
        id: u32,
        coordinate: &RecordCoordinate,
        new_key: &str,
    ) -> Result<RenameRecordOutcome, EditorError> {
        let entry = self.session(id)?;
        let session_lock = &entry.state;
        let mut session = session_lock.write();
        session.ensure_writable()?;
        let report = coflow_project::commands::apply_project_mutation(
            &mut session.project_session,
            MutationRequest {
                stop_on_write_error: true,
                ops: vec![MutationOp::RenameRecord {
                    record: coordinate.clone(),
                    file: None,
                    new_key: new_key.to_string(),
                }],
            },
        )
        .map_err(api_diagnostics_to_editor_error)?;
        let (report, changes) = finalize_mutation(&mut session, report, "rename record failed")?;
        let outcome = report
            .applied
            .first()
            .map(|applied| applied.outcome.clone())
            .ok_or_else(|| EditorError::write("rename did not apply"))?;
        let renamed = outcome
            .renamed
            .and_then(|(old, new)| (old == *coordinate).then_some(new))
            .ok_or_else(|| EditorError::write("rename did not produce a new coordinate"))?;
        Ok(RenameRecordOutcome {
            changes,
            revision: session.revisions.current(),
            diagnostics: report.diagnostics,
            renamed,
            affected_files: report.affected_files,
        })
    }

    pub fn delete_record(
        &self,
        id: u32,
        coordinate: &RecordCoordinate,
    ) -> Result<DeleteRecordOutcome, EditorError> {
        let entry = self.session(id)?;
        let session_lock = &entry.state;
        let mut session = session_lock.write();
        session.ensure_writable()?;
        let deleted_snapshot = snapshot_record_before_delete(&session, coordinate);
        let report = coflow_project::commands::apply_project_mutation(
            &mut session.project_session,
            MutationRequest {
                stop_on_write_error: true,
                ops: vec![MutationOp::DeleteRecord {
                    record: coordinate.clone(),
                    file: None,
                }],
            },
        )
        .map_err(api_diagnostics_to_editor_error)?;
        let (report, changes) = finalize_mutation(&mut session, report, "delete record failed")?;
        Ok(DeleteRecordOutcome {
            changes,
            revision: session.revisions.current(),
            diagnostics: report.diagnostics,
            affected_files: report.affected_files,
            deleted_snapshot,
        })
    }

    pub fn swap_records(
        &self,
        id: u32,
        first: &RecordCoordinate,
        second: &RecordCoordinate,
    ) -> Result<ReorderRecordsOutcome, EditorError> {
        let entry = self.session(id)?;
        let mut session = entry.state.write();
        session.ensure_writable()?;
        let file_path = reorder_file_path(&session, first)?;
        let report = coflow_project::commands::apply_project_mutation(
            &mut session.project_session,
            MutationRequest {
                stop_on_write_error: true,
                ops: vec![MutationOp::SwapRecords {
                    first: first.clone(),
                    second: second.clone(),
                    file: Some(file_path.clone()),
                }],
            },
        )
        .map_err(api_diagnostics_to_editor_error)?;
        let (report, changes) = finalize_mutation(&mut session, report, "swap records failed")?;
        Ok(ReorderRecordsOutcome {
            changes,
            revision: session.revisions.current(),
            diagnostics: report.diagnostics,
            affected_files: report.affected_files,
            old_index: None,
            new_index: None,
        })
    }

    pub fn move_record(
        &self,
        id: u32,
        coordinate: &RecordCoordinate,
        target_index: usize,
    ) -> Result<ReorderRecordsOutcome, EditorError> {
        let entry = self.session(id)?;
        let mut session = entry.state.write();
        session.ensure_writable()?;
        let file_path = reorder_file_path(&session, coordinate)?;
        let old_index = record_container_index(&session, coordinate).ok_or_else(|| {
            EditorError::not_found(format!(
                "record `{}.{}` not found in source order",
                coordinate.actual_type, coordinate.key
            ))
        })?;
        let report = coflow_project::commands::apply_project_mutation(
            &mut session.project_session,
            MutationRequest {
                stop_on_write_error: true,
                ops: vec![MutationOp::MoveRecord {
                    record: coordinate.clone(),
                    target_index,
                    file: Some(file_path.clone()),
                }],
            },
        )
        .map_err(api_diagnostics_to_editor_error)?;
        let (report, changes) = finalize_mutation(&mut session, report, "move record failed")?;
        Ok(ReorderRecordsOutcome {
            changes,
            revision: session.revisions.current(),
            diagnostics: report.diagnostics,
            affected_files: report.affected_files,
            old_index: Some(old_index),
            new_index: Some(target_index),
        })
    }

    pub fn transfer_record(
        &self,
        id: u32,
        coordinate: &RecordCoordinate,
        destination_file: &str,
        target_index: usize,
    ) -> Result<ReorderRecordsOutcome, EditorError> {
        let entry = self.session(id)?;
        let mut session = entry.state.write();
        session.ensure_writable()?;
        let source_file = reorder_file_path(&session, coordinate)?;
        let old_index = record_type_index(&session, coordinate).ok_or_else(|| {
            EditorError::not_found(format!(
                "record `{}.{}` not found in source type order",
                coordinate.actual_type, coordinate.key
            ))
        })?;
        let report = coflow_project::commands::apply_project_mutation(
            &mut session.project_session,
            MutationRequest {
                stop_on_write_error: true,
                ops: vec![MutationOp::TransferRecord {
                    record: coordinate.clone(),
                    destination_file: destination_file.to_string(),
                    target_index,
                    source_file: Some(source_file),
                }],
            },
        )
        .map_err(api_diagnostics_to_editor_error)?;
        let (report, changes) = finalize_mutation(&mut session, report, "transfer record failed")?;
        Ok(ReorderRecordsOutcome {
            changes,
            revision: session.revisions.current(),
            diagnostics: report.diagnostics,
            affected_files: report.affected_files,
            old_index: Some(old_index),
            new_index: Some(target_index),
        })
    }
}
