//! Record queries and mutation commands for loaded editor sessions.

use super::*;

impl SessionStore {
    pub fn get_file_records(&self, id: u32, file_path: &str) -> Result<FileRecords, EditorError> {
        let entry = self.session(id)?;
        let session_lock = &entry.state;
        let session = session_lock
            .read()
            .map_err(|_| EditorError::session("session poisoned"))?;
        Ok(file_records_for_session(&session, file_path))
    }

    pub fn make_default_object(&self, id: u32, type_name: &str) -> Result<CfdValue, EditorError> {
        let entry = self.session(id)?;
        let session_lock = &entry.state;
        let session = session_lock
            .read()
            .map_err(|_| EditorError::session("session poisoned"))?;
        session
            .engine
            .default_record_value(type_name, DefaultMaterialization::EditableShape)
            .map_err(api_diagnostics_to_editor_error)
    }

    pub fn create_record_draft(
        &self,
        id: u32,
        actual_type: &str,
    ) -> Result<CreateRecordDraft, EditorError> {
        let entry = self.session(id)?;
        let session_lock = &entry.state;
        let session = session_lock
            .read()
            .map_err(|_| EditorError::session("session poisoned"))?;
        let draft = session
            .engine
            .create_record_draft(actual_type)
            .map_err(api_diagnostics_to_editor_error)?;
        let ctx = WireContext::new(session.queries(), &session.diagnostics);
        let wire = create_record_draft_to_wire(&draft, &ctx);
        drop(session);
        Ok(wire)
    }

    pub fn render_cell_text(
        &self,
        id: u32,
        coordinate: &RecordCoordinate,
        field_path: &[coflow_data_model::CfdPathSegment],
    ) -> Result<String, EditorError> {
        let entry = self.session(id)?;
        let session = entry
            .state
            .read()
            .map_err(|_| EditorError::session("session poisoned"))?;
        session
            .engine
            .render_cell_text(coordinate, field_path)
            .map_err(api_diagnostics_to_editor_error)
    }

    pub fn parse_cell_text(
        &self,
        id: u32,
        coordinate: &RecordCoordinate,
        field_path: &[coflow_data_model::CfdPathSegment],
        text: &str,
    ) -> Result<CfdValue, EditorError> {
        let entry = self.session(id)?;
        let session = entry
            .state
            .read()
            .map_err(|_| EditorError::session("session poisoned"))?;
        session
            .engine
            .parse_cell_text(coordinate, field_path, text)
            .map_err(api_diagnostics_to_editor_error)
    }

    pub fn get_enum_variants(&self, id: u32, enum_name: &str) -> Result<Vec<String>, EditorError> {
        let entry = self.session(id)?;
        let session_lock = &entry.state;
        let session = session_lock
            .read()
            .map_err(|_| EditorError::session("session poisoned"))?;
        Ok(session.queries().enum_variants(enum_name))
    }

    /// Records assignable to `expected_type`, surfaced as `RefTarget`s so
    /// the front-end can render `Type.key` and jump directly.
    pub fn get_ref_targets(
        &self,
        id: u32,
        expected_type: &str,
    ) -> Result<Vec<RefTarget>, EditorError> {
        let entry = self.session(id)?;
        let session_lock = &entry.state;
        let targets = {
            let mut session = session_lock
                .write()
                .map_err(|_| EditorError::session("session poisoned"))?;
            if let Some(cached) = session.ref_target_cache.get(expected_type) {
                return Ok(cached.clone());
            }
            let targets: Vec<RefTarget> = session
                .queries()
                .ref_targets(expected_type)
                .into_iter()
                .map(|target| RefTarget {
                    coordinate: target.coordinate,
                    file_path: target.file_path,
                })
                .collect();
            session
                .ref_target_cache
                .insert(expected_type.to_string(), targets.clone());
            targets
        };
        Ok(targets)
    }

    pub fn get_graph(&self, id: u32, query: &GraphQuery) -> Result<GraphData, EditorError> {
        let entry = self.session(id)?;
        let session_lock = &entry.state;
        let session = session_lock
            .read()
            .map_err(|_| EditorError::session("session poisoned"))?;
        Ok(graph::build_graph(&session, query))
    }

    /// Persist a single field edit addressed by its owner record coordinate.
    #[allow(clippy::too_many_lines)]
    pub fn write_field(
        &self,
        id: u32,
        coordinate: &RecordCoordinate,
        field_path: &[coflow_data_model::CfdPathSegment],
        new_value: &CfdValue,
    ) -> Result<WriteFieldOutcome, EditorError> {
        let entry = self.session(id)?;
        let mut session = entry
            .state
            .write()
            .map_err(|_| EditorError::session("session poisoned"))?;
        write_field_in_session(&mut session, coordinate, field_path, new_value)
    }

    pub fn edit_collection(
        &self,
        id: u32,
        coordinate: &RecordCoordinate,
        field_path: &[coflow_data_model::CfdPathSegment],
        edit: CollectionEdit,
    ) -> Result<WriteFieldOutcome, EditorError> {
        let entry = self.session(id)?;
        let mut session = entry
            .state
            .write()
            .map_err(|_| EditorError::session("session poisoned"))?;
        let current = session
            .queries()
            .field_value(&coordinate.actual_type, &coordinate.key, field_path)
            .cloned()
            .ok_or_else(|| EditorError::not_found("collection field not found"))?;
        let default_item = session
            .engine
            .default_collection_item_value(&coordinate.actual_type, field_path)
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

        let mut session = session_lock
            .write()
            .map_err(|_| EditorError::session("session poisoned"))?;
        let report = session.engine.apply_mutation(MutationRequest {
            stop_on_write_error: true,
            ops: vec![MutationOp::InsertRecord {
                file: file_path.to_string(),
                sheet: None,
                actual_type: actual_type.to_string(),
                key: record_key.to_string(),
                fields: MutationFields::Cfd(fields_map),
                materialization,
            }],
        });
        let report = finalize_mutation(&mut session, report, "insert record failed")?;
        let file_records = file_records_for_session(&session, file_path);
        Ok(InsertRecordOutcome {
            revision: session.revisions.current(),
            file_records,
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
        let mut session = session_lock
            .write()
            .map_err(|_| EditorError::session("session poisoned"))?;
        let report = session.engine.apply_mutation(MutationRequest {
            stop_on_write_error: true,
            ops: vec![MutationOp::RenameRecord {
                record: coordinate.clone(),
                file: None,
                new_key: new_key.to_string(),
            }],
        });
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
            .record_view(&renamed.actual_type, &renamed.key)
            .ok_or_else(|| {
                EditorError::not_found(format!(
                    "record `{}.{}` not found after rename",
                    renamed.actual_type, renamed.key
                ))
            })?;
        let ctx = WireContext::new(session.queries(), &session.diagnostics);
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
        let mut session = session_lock
            .write()
            .map_err(|_| EditorError::session("session poisoned"))?;
        let deleted_snapshot = snapshot_record_before_delete(&session, coordinate);
        let file_path = deleted_snapshot
            .as_ref()
            .map(|snapshot| snapshot.display_path.clone())
            .or_else(|| {
                session
                    .queries()
                    .file_for_record(&coordinate.actual_type, &coordinate.key)
                    .map(str::to_string)
            })
            .ok_or_else(|| {
                EditorError::not_found(format!(
                    "record `{}.{}` not found",
                    coordinate.actual_type, coordinate.key
                ))
            })?;
        let report = session.engine.apply_mutation(MutationRequest {
            stop_on_write_error: true,
            ops: vec![MutationOp::DeleteRecord {
                record: coordinate.clone(),
                file: None,
            }],
        });
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
        let mut session = entry
            .state
            .write()
            .map_err(|_| EditorError::session("session poisoned"))?;
        let file_path = reorder_file_path(&session, first)?;
        let report = session.engine.apply_mutation(MutationRequest {
            stop_on_write_error: true,
            ops: vec![MutationOp::SwapRecords {
                first: first.clone(),
                second: second.clone(),
                file: Some(file_path.clone()),
            }],
        });
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
        let mut session = entry
            .state
            .write()
            .map_err(|_| EditorError::session("session poisoned"))?;
        let file_path = reorder_file_path(&session, coordinate)?;
        let old_index = record_container_index(&session, coordinate).ok_or_else(|| {
            EditorError::not_found(format!(
                "record `{}.{}` not found in source order",
                coordinate.actual_type, coordinate.key
            ))
        })?;
        let report = session.engine.apply_mutation(MutationRequest {
            stop_on_write_error: true,
            ops: vec![MutationOp::MoveRecord {
                record: coordinate.clone(),
                target_index,
                file: Some(file_path.clone()),
            }],
        });
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
        destination_sheet: Option<&str>,
        target_index: usize,
    ) -> Result<ReorderRecordsOutcome, EditorError> {
        let entry = self.session(id)?;
        let mut session = entry
            .state
            .write()
            .map_err(|_| EditorError::session("session poisoned"))?;
        let source_file = reorder_file_path(&session, coordinate)?;
        let old_index = record_type_index(&session, coordinate).ok_or_else(|| {
            EditorError::not_found(format!(
                "record `{}.{}` not found in source type order",
                coordinate.actual_type, coordinate.key
            ))
        })?;
        let report = session.engine.apply_mutation(MutationRequest {
            stop_on_write_error: true,
            ops: vec![MutationOp::TransferRecord {
                record: coordinate.clone(),
                destination_file: destination_file.to_string(),
                destination_sheet: destination_sheet.map(ToOwned::to_owned),
                target_index,
                source_file: Some(source_file),
            }],
        });
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
