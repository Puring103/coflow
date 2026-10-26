//! Mutation 命令：字段写回、集合编辑、记录增删改与排序。
//!
//! 引擎拥有校验与落盘，本层只做 `MutationReport` 到 wire DTO 的包装。

use super::super::errors::api_diagnostics_to_editor_error;
use super::super::{
    mutation_apply::{apply_collection_edit, finalize_mutation, write_field_in_session},
    row_build::{
        file_records_for_session, record_container_index, record_type_index, reorder_file_path,
        snapshot_record_before_delete,
    },
    SessionStore,
};
use crate::editor::convert::{record_view_to_row, WireContext};
use crate::editor::types::{
    BatchWriteFieldEditOutcome, BatchWriteFieldInput, BatchWriteFieldOutcome, CollectionEdit,
    DeleteRecordOutcome, EditorError, InsertRecordOutcome, RenameRecordOutcome,
    ReorderRecordsOutcome, WriteFieldOutcome,
};
use coflow_runtime::{
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
        field_path: &[coflow_runtime::CfdPathSegment],
        new_value: &CfdValue,
    ) -> Result<WriteFieldOutcome, EditorError> {
        let entry = self.session(id)?;
        let mut session = entry.state.write();
        session.ensure_writable()?;
        let outcome = write_field_in_session(&mut session, coordinate, field_path, new_value);
        drop(session);
        outcome
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
        let report = coflow_runtime::commands::apply_project_mutation(
            &mut session.engine,
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
        let report = finalize_mutation(&mut session, report, "batch field write failed")?;
        // 结果已按请求下标排序，顺序游标避免每个单元格重复扫描完整批次。
        let mut applied = report.applied.iter().peekable();
        let edits = targets
            .into_iter()
            .enumerate()
            .map(|(index, (write, old_value))| {
                while applied.peek().is_some_and(|item| item.index < index) {
                    applied.next();
                }
                let final_coordinate = applied
                    .peek()
                    .filter(|item| item.index == index)
                    .and_then(|applied| applied.outcome.renamed.as_ref())
                    .and_then(|(old, new)| (old == &write.coordinate).then(|| new.clone()))
                    .unwrap_or_else(|| write.coordinate.clone());
                let new_value = session
                    .queries()
                    .field_value(
                        &final_coordinate.actual_type,
                        &final_coordinate.id,
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
        field_path: &[coflow_runtime::CfdPathSegment],
        edit: CollectionEdit,
    ) -> Result<WriteFieldOutcome, EditorError> {
        let entry = self.session(id)?;
        let mut session = entry.state.write();
        session.ensure_writable()?;
        let current = session
            .queries()
            .field_value(&coordinate.actual_type, &coordinate.id, field_path)
            .cloned()
            .ok_or_else(|| EditorError::not_found("collection field not found"))?;
        let default_item = session
            .engine
            .default_collection_item_value_for_record(coordinate, field_path)
            .ok();
        let next = apply_collection_edit(current, edit, default_item)?;
        let outcome = write_field_in_session(&mut session, coordinate, field_path, &next);
        drop(session);
        outcome
    }

    pub fn insert_record(
        &self,
        id: u32,
        file_path: &str,
        record_id: &str,
        actual_type: &str,
        fields: CfdValue,
    ) -> Result<InsertRecordOutcome, EditorError> {
        self.insert_record_with_materialization(
            id,
            file_path,
            record_id,
            actual_type,
            fields,
            DefaultMaterialization::Minimal,
        )
    }

    pub fn insert_record_with_materialization(
        &self,
        id: u32,
        file_path: &str,
        record_id: &str,
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
        let report = coflow_runtime::commands::apply_project_mutation(
            &mut session.engine,
            MutationRequest {
                stop_on_write_error: true,
                ops: vec![MutationOp::InsertRecord {
                    file: file_path.to_string(),
                    actual_type: actual_type.to_string(),
                    id: record_id.to_string(),
                    fields: MutationFields::Cfd(fields_map),
                    materialization,
                }],
            },
        )
        .map_err(api_diagnostics_to_editor_error)?;
        let report = finalize_mutation(&mut session, report, "insert record failed")?;
        let file_records = file_records_for_session(&session, file_path);
        Ok(InsertRecordOutcome {
            revision: session.revisions.current(),
            file_records,
            diagnostics: report.diagnostics,
            affected_files: report.affected_files,
        })
    }

    pub fn rename_record_id(
        &self,
        id: u32,
        coordinate: &RecordCoordinate,
        new_id: &str,
    ) -> Result<RenameRecordOutcome, EditorError> {
        let entry = self.session(id)?;
        let session_lock = &entry.state;
        let mut session = session_lock.write();
        session.ensure_writable()?;
        let report = coflow_runtime::commands::apply_project_mutation(
            &mut session.engine,
            MutationRequest {
                stop_on_write_error: true,
                ops: vec![MutationOp::RenameRecord {
                    record: coordinate.clone(),
                    file: None,
                    new_id: new_id.to_string(),
                }],
            },
        )
        .map_err(api_diagnostics_to_editor_error)?;
        let report = finalize_mutation(&mut session, report, "rename record failed")?;
        let outcome = report
            .applied
            .first()
            .map(|applied| applied.outcome.clone())
            .ok_or_else(|| EditorError::write("rename did not apply"))?;
        let renamed = outcome
            .renamed
            .and_then(|(old, new)| (old == *coordinate).then_some(new))
            .ok_or_else(|| EditorError::write("rename did not produce a new coordinate"))?;
        let view = session
            .queries()
            .record_view(&renamed.actual_type, &renamed.id)
            .ok_or_else(|| {
                EditorError::not_found(format!(
                    "record `{}.{}` not found after rename",
                    renamed.actual_type, renamed.id
                ))
            })?;
        let ctx = WireContext::new(
            session.queries(),
            &session.diagnostics,
            &session.shape_cache,
        );
        let row = record_view_to_row(&view, &ctx);
        Ok(RenameRecordOutcome {
            revision: session.revisions.current(),
            row,
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
        let file_path = deleted_snapshot
            .as_ref()
            .map(|snapshot| snapshot.display_path.clone())
            .or_else(|| {
                session
                    .queries()
                    .file_for_record(&coordinate.actual_type, &coordinate.id)
                    .map(str::to_string)
            })
            .ok_or_else(|| {
                EditorError::not_found(format!(
                    "record `{}.{}` not found",
                    coordinate.actual_type, coordinate.id
                ))
            })?;
        let report = coflow_runtime::commands::apply_project_mutation(
            &mut session.engine,
            MutationRequest {
                stop_on_write_error: true,
                ops: vec![MutationOp::DeleteRecord {
                    record: coordinate.clone(),
                    file: None,
                }],
            },
        )
        .map_err(api_diagnostics_to_editor_error)?;
        let report = finalize_mutation(&mut session, report, "delete record failed")?;
        let file_records = file_records_for_session(&session, &file_path);
        Ok(DeleteRecordOutcome {
            revision: session.revisions.current(),
            file_records,
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
        let report = coflow_runtime::commands::apply_project_mutation(
            &mut session.engine,
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
        let report = finalize_mutation(&mut session, report, "swap records failed")?;
        Ok(ReorderRecordsOutcome {
            revision: session.revisions.current(),
            file_records: file_records_for_session(&session, &file_path),
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
                coordinate.actual_type, coordinate.id
            ))
        })?;
        let report = coflow_runtime::commands::apply_project_mutation(
            &mut session.engine,
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
        let report = finalize_mutation(&mut session, report, "move record failed")?;
        Ok(ReorderRecordsOutcome {
            revision: session.revisions.current(),
            file_records: file_records_for_session(&session, &file_path),
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
                coordinate.actual_type, coordinate.id
            ))
        })?;
        let report = coflow_runtime::commands::apply_project_mutation(
            &mut session.engine,
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
        let report = finalize_mutation(&mut session, report, "transfer record failed")?;
        Ok(ReorderRecordsOutcome {
            revision: session.revisions.current(),
            file_records: file_records_for_session(&session, destination_file),
            diagnostics: report.diagnostics,
            affected_files: report.affected_files,
            old_index: Some(old_index),
            new_index: Some(target_index),
        })
    }
}

impl SessionStore {
    /// 将复合数据操作整体校验、暂存、发布，失败不推进任何历史条目。
    pub fn apply_mutation_batch(
        &self,
        id: u32,
        ops: Vec<MutationOp>,
    ) -> Result<crate::editor::types::BatchMutationOutcome, EditorError> {
        let entry = self.session(id)?;
        let mut session = entry.state.write();
        session.ensure_writable()?;
        let report = coflow_runtime::commands::apply_project_mutation(
            &mut session.engine,
            MutationRequest {
                stop_on_write_error: true,
                ops,
            },
        )
        .map_err(api_diagnostics_to_editor_error)?;
        let report = finalize_mutation(&mut session, report, "批量编辑失败")?;
        Ok(crate::editor::types::BatchMutationOutcome {
            revision: session.revisions.current(),
            renamed: report
                .applied
                .iter()
                .filter_map(|op| op.outcome.renamed.clone())
                .collect(),
            diagnostics: report.diagnostics,
            affected_files: report.affected_files,
        })
    }
}

impl SessionStore {
    /// 新建目标与建立引用必须在同一事务中，连线失败时不能留下孤立的新记录。
    pub fn create_linked_record(
        &self,
        id: u32,
        input: crate::editor::types::CreateLinkedRecordInput,
    ) -> Result<crate::editor::types::LinkedRecordOutcome, EditorError> {
        let entry = self.session(id)?;
        let mut session = entry.state.write();
        session.ensure_writable()?;
        let old_value = session
            .queries()
            .field_value(
                &input.source.actual_type,
                &input.source.id,
                &input.field_path,
            )
            .cloned()
            .ok_or_else(|| EditorError::not_found("引用来源字段不存在，请刷新图谱后重试"))?;
        let new_value = if input.append {
            apply_collection_edit(
                old_value.clone(),
                CollectionEdit::ArrayAppend {
                    value: Some(input.reference),
                },
                None,
            )?
        } else {
            input.reference
        };
        let CfdValue::Object(fields) = input.fields else {
            return Err(EditorError::write("新记录字段必须为对象"));
        };
        let report = coflow_runtime::commands::apply_project_mutation(
            &mut session.engine,
            MutationRequest {
                stop_on_write_error: true,
                ops: vec![
                    MutationOp::InsertRecord {
                        file: input.file,
                        actual_type: input.actual_type,
                        id: input.id,
                        fields: MutationFields::Cfd(
                            fields
                                .fields
                                .into_iter()
                                .map(|(key, value)| (key.to_string(), value))
                                .collect(),
                        ),
                        materialization: DefaultMaterialization::Minimal,
                    },
                    MutationOp::SetField {
                        record: input.source,
                        file: Some(input.source_file),
                        path: input.field_path,
                        value: MutationValue::Cfd(new_value.clone()),
                    },
                ],
            },
        )
        .map_err(api_diagnostics_to_editor_error)?;
        let report = finalize_mutation(&mut session, report, "新建并连线失败")?;
        Ok(crate::editor::types::LinkedRecordOutcome {
            revision: session.revisions.current(),
            diagnostics: report.diagnostics,
            affected_files: report.affected_files,
            old_value,
            new_value,
        })
    }
}
