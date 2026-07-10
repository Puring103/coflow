//! Write transaction surface on `ProjectSession`.
//!
//! Hosts call `session.write_field(...)` / `insert_record` / `delete_record`
//! with stable `(actual_type, key)` coordinates. The engine resolves the
//! coordinate to an internal record id, dispatches the edit to the
//! registered writer, then rebuilds itself in place so subsequent queries
//! see the post-write state.

mod path;
mod refs;
mod target;
mod transaction;
mod writer;

use coflow_api::{
    DeleteRecordRequest, Diagnostic, DiagnosticSet, InsertRecordRequest, ProviderRegistry,
    RecordOrigin, RenameRecordRequest, WriteCellRequest, WriteContext, WriteFieldPathSegment,
};
use coflow_cft::CftSchemaView;
use coflow_data_model::{CfdPath, CfdRecord, CfdRecordId, CfdValue};

use super::records::WriteOutcome;
use super::write_rules;
use super::{build_project_session_for_build, ProjectSession, RecordCoordinate};
use refs::{reference_update_actions, source_rewrite_actions};
use target::{guess_new_coordinate, is_id_path, write_target_for_path};
use transaction::LocalFileTransaction;
use writer::{lookup_source_writer, source_for_file};

pub(crate) fn record_value_at_path<'a>(
    record: &'a CfdRecord,
    path: &CfdPath,
) -> Option<&'a CfdValue> {
    path::value_at_path(record, path)
}

impl ProjectSession {
    /// Persist a single field edit and rebuild the session in place.
    ///
    /// `actual_type` + `key` identify the host record. The writer
    /// preflights before mutating the source — diagnostics from preflight
    /// are returned without rebuilding.
    ///
    /// On success the engine triggers `build_project_session_for_build` again to
    /// refresh model, diagnostics, and indexes. The
    /// [`WriteOutcome`] reports the post-write coordinate (which differs
    /// when the write changed the host record's `id` field).
    ///
    /// # Errors
    ///
    /// Returns a [`DiagnosticSet`] when the record is unknown, no writer is
    /// registered for the host provider, preflight reports a problem, the
    /// writer rejects the edit, or the post-write rebuild fails.
    pub fn write_field(
        &mut self,
        registry: &ProviderRegistry,
        actual_type: &str,
        key: &str,
        path: &[WriteFieldPathSegment],
        new_value: &CfdValue,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        if is_id_path(path) {
            let CfdValue::String(new_key) = new_value else {
                return Err(DiagnosticSet::one(Diagnostic::error(
                    "WRITE-RENAME",
                    "WRITE",
                    "record key writes require a string value",
                )));
            };
            return self.rename_record_key(registry, actual_type, key, new_key);
        }
        let Some(record_ref) = self.records.get_by_coordinate(actual_type, key) else {
            return Err(DiagnosticSet::one(not_found(actual_type, key)));
        };
        let Some(_record) = self.model.record(record_ref.id) else {
            return Err(DiagnosticSet::one(not_found(actual_type, key)));
        };
        let coordinate = record_ref.coordinate.clone();
        let target = write_target_for_path(self, record_ref, path)?;
        let expected = write_rules::expected_type_for_write_path(
            &self.schema,
            &target.coordinate.actual_type,
            &target.field_path,
            "WRITE-SHAPE",
            "WRITE",
        )?;
        write_rules::validate_value_for_write(self, &expected, new_value, "WRITE-SHAPE", "WRITE")?;
        let source = source_for_file(self, &target.display_path)?;
        let writer = lookup_source_writer(registry, &source)?;
        let yaml_path = self.project.config_path.clone();

        let write_request = WriteCellRequest {
            origin: &target.origin,
            record_key: &target.coordinate.key,
            actual_type: &target.coordinate.actual_type,
            field_path: &target.field_path,
            new_value,
            schema: &self.schema,
            source: &source,
        };
        let write_ctx = WriteContext {
            project_root: &self.project.root_dir,
            schema: &self.schema,
            model: Some(&self.model),
        };
        let preflight = writer.preflight(write_ctx, &write_request);
        if !preflight.is_empty() {
            return Err(preflight);
        }
        writer.write_field(write_ctx, &write_request)?;

        let new_session = build_project_session_for_build(self.project.clone(), registry)?;
        let new_write_coordinate =
            guess_new_coordinate(&new_session, &target.coordinate, path, new_value);
        let renamed = (new_write_coordinate != target.coordinate)
            .then(|| (target.coordinate.clone(), new_write_coordinate.clone()));
        let diagnostics = new_session.diagnostics.as_set().clone();
        *self = new_session;
        let _ = yaml_path;
        let mut touched = vec![coordinate.clone()];
        if new_write_coordinate != coordinate {
            touched.push(new_write_coordinate);
        }
        Ok(WriteOutcome {
            touched,
            inserted: None,
            deleted: None,
            renamed,
            diagnostics,
        })
    }

    /// Rename a top-level record key and update references across loaded
    /// sources before rebuilding the session.
    ///
    /// # Errors
    /// Returns diagnostics when the record is unknown, the new key is invalid
    /// or collides in the target type/range, any affected source has no writer,
    /// a writer rejects one of the edits, or the post-write rebuild fails.
    pub fn rename_record_key(
        &mut self,
        registry: &ProviderRegistry,
        actual_type: &str,
        old_key: &str,
        new_key: &str,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        validate_new_record_key(new_key)?;
        let Some(target_ref) = self.records.get_by_coordinate(actual_type, old_key) else {
            return Err(DiagnosticSet::one(not_found(actual_type, old_key)));
        };
        if old_key == new_key {
            return Ok(WriteOutcome::touch(target_ref.coordinate.clone()));
        }
        ensure_rename_key_available(self, actual_type, new_key, target_ref.id)?;

        let target_id = target_ref.id;
        let old_coordinate = target_ref.coordinate.clone();
        let target_origin = target_ref.origin.clone();
        let target_display_path = target_ref.display_path.clone();
        let target_source = source_for_file(self, &target_display_path)?;
        let target_writer = lookup_source_writer(registry, &target_source)?;
        let ctx = WriteContext {
            project_root: &self.project.root_dir,
            schema: &self.schema,
            model: Some(&self.model),
        };
        let target_request = RenameRecordRequest {
            origin: &target_origin,
            old_key,
            new_key,
            actual_type,
            source: &target_source,
            schema: &self.schema,
        };

        let reference_actions = reference_update_actions(self, registry, target_id, new_key)?;
        let rewrite_actions = source_rewrite_actions(self, registry, target_id, old_key, new_key)?;
        let transaction = LocalFileTransaction::begin(
            std::iter::once(&target_source)
                .chain(reference_actions.iter().map(|action| action.source()))
                .chain(rewrite_actions.iter().map(|action| action.source())),
        )?;

        if let Err(mut diagnostics) = target_writer.rename_record(ctx, &target_request) {
            rollback_transaction(transaction, &mut diagnostics);
            return Err(diagnostics);
        }
        for action in &reference_actions {
            let request = action.request.as_request(&self.schema);
            if let Err(mut diagnostics) = action.writer.write_field(ctx, &request) {
                rollback_transaction(transaction, &mut diagnostics);
                return Err(diagnostics);
            }
        }
        for action in &rewrite_actions {
            let request = action.request.as_request(&self.schema);
            if let Err(mut diagnostics) = action.writer.rewrite_record_references(ctx, &request) {
                rollback_transaction(transaction, &mut diagnostics);
                return Err(diagnostics);
            }
        }

        let new_session = match build_project_session_for_build(self.project.clone(), registry) {
            Ok(session) => session,
            Err(mut diagnostics) => {
                rollback_transaction(transaction, &mut diagnostics);
                return Err(diagnostics);
            }
        };
        let new_coordinate = RecordCoordinate::new(actual_type, new_key);
        let diagnostics = new_session.diagnostics.as_set().clone();
        *self = new_session;
        Ok(WriteOutcome {
            touched: vec![old_coordinate.clone(), new_coordinate.clone()],
            inserted: None,
            deleted: None,
            renamed: Some((old_coordinate, new_coordinate)),
            diagnostics,
        })
    }

    /// Persist a new top-level record and rebuild the session.
    ///
    /// # Errors
    ///
    /// Returns a [`DiagnosticSet`] when the file is unknown, no writer is
    /// registered, the writer rejects insertion, or the rebuild fails.
    pub fn insert_record(
        &mut self,
        registry: &ProviderRegistry,
        file: &str,
        sheet: Option<&str>,
        record_key: &str,
        actual_type: &str,
        fields: &std::collections::BTreeMap<String, CfdValue>,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        let source = source_for_file(self, file)?;
        ensure_insert_type_can_insert(self, actual_type)?;
        ensure_insert_key_available(self, actual_type, record_key)?;
        validate_insert_fields(self, actual_type, record_key, fields)?;
        let sheet = sheet
            .map(ToOwned::to_owned)
            .or_else(|| sheet_for_file_type(self, file, actual_type));
        let writer = lookup_source_writer(registry, &source)?;
        let request = InsertRecordRequest {
            source: &source,
            sheet: sheet.as_deref(),
            record_key,
            actual_type,
            fields,
            schema: &self.schema,
        };
        let ctx = WriteContext {
            project_root: &self.project.root_dir,
            schema: &self.schema,
            model: Some(&self.model),
        };
        writer.insert_record(ctx, &request)?;

        let new_session = build_project_session_for_build(self.project.clone(), registry)?;
        let inserted = RecordCoordinate::new(actual_type, record_key);
        let diagnostics = new_session.diagnostics.as_set().clone();
        *self = new_session;
        Ok(WriteOutcome {
            touched: vec![inserted.clone()],
            inserted: Some(inserted),
            deleted: None,
            renamed: None,
            diagnostics,
        })
    }

    /// Delete a top-level record and rebuild the session.
    ///
    /// # Errors
    ///
    /// Returns a [`DiagnosticSet`] when the record is unknown, no writer is
    /// registered, the writer rejects deletion, or the rebuild fails.
    pub fn delete_record(
        &mut self,
        registry: &ProviderRegistry,
        actual_type: &str,
        key: &str,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        let Some(record_ref) = self.records.get_by_coordinate(actual_type, key) else {
            return Err(DiagnosticSet::one(not_found(actual_type, key)));
        };
        let Some(record) = self.model.record(record_ref.id) else {
            return Err(DiagnosticSet::one(not_found(actual_type, key)));
        };
        let coordinate = record_ref.coordinate.clone();
        let display_path = record_ref.display_path.clone();
        let origin = record.origin.clone();
        let source = source_for_file(self, &display_path)?;
        let writer = lookup_source_writer(registry, &source)?;
        let request = DeleteRecordRequest {
            origin: &origin,
            record_key: key,
            actual_type,
            source: &source,
        };
        let ctx = WriteContext {
            project_root: &self.project.root_dir,
            schema: &self.schema,
            model: Some(&self.model),
        };
        writer.delete_record(ctx, &request)?;

        let new_session = build_project_session_for_build(self.project.clone(), registry)?;
        let diagnostics = new_session.diagnostics.as_set().clone();
        *self = new_session;
        Ok(WriteOutcome {
            touched: Vec::new(),
            inserted: None,
            deleted: Some(coordinate),
            renamed: None,
            diagnostics,
        })
    }
}

fn rollback_transaction(
    transaction: Option<LocalFileTransaction>,
    diagnostics: &mut DiagnosticSet,
) {
    if let Some(transaction) = transaction {
        transaction.rollback_into(diagnostics);
    }
}

fn not_found(actual_type: &str, key: &str) -> Diagnostic {
    Diagnostic::error(
        "WRITE-NOT-FOUND",
        "WRITE",
        format!("record `{actual_type}.{key}` was not found in the session"),
    )
}

/// Pick the sheet name to target when inserting a record into a table source.
fn sheet_for_file_type(session: &ProjectSession, file: &str, actual_type: &str) -> Option<String> {
    for id in session.records.ids_in_file(file) {
        let Some(record_ref) = session.records.get(*id) else {
            continue;
        };
        let RecordOrigin::Table { sheet, .. } = &record_ref.origin else {
            continue;
        };
        if record_ref.coordinate.actual_type == actual_type {
            return Some(sheet.clone());
        }
    }
    None
}

fn validate_new_record_key(key: &str) -> Result<(), DiagnosticSet> {
    write_rules::validate_record_key(key, "WRITE-RENAME")
}

fn ensure_insert_key_available(
    session: &ProjectSession,
    actual_type: &str,
    key: &str,
) -> Result<(), DiagnosticSet> {
    write_rules::ensure_record_key_available(
        session,
        actual_type,
        key,
        None,
        "WRITE-INSERT",
        "WRITE",
    )
}

fn ensure_insert_type_can_insert(
    session: &ProjectSession,
    actual_type: &str,
) -> Result<(), DiagnosticSet> {
    let schema_view = CftSchemaView::new(&session.schema);
    let Some(schema_type) = schema_view.type_meta(actual_type) else {
        return Err(DiagnosticSet::one(Diagnostic::error(
            "WRITE-INSERT",
            "WRITE",
            format!("unknown insert type `{actual_type}`"),
        )));
    };
    if schema_type.is_abstract {
        return Err(DiagnosticSet::one(Diagnostic::error(
            "WRITE-INSERT",
            "WRITE",
            format!("abstract type `{actual_type}` cannot be inserted"),
        )));
    }
    if schema_type.is_singleton {
        return Err(DiagnosticSet::one(Diagnostic::error(
            "WRITE-INSERT",
            "WRITE",
            format!("singleton type `{actual_type}` cannot be inserted"),
        )));
    }
    Ok(())
}

fn validate_insert_fields(
    session: &ProjectSession,
    actual_type: &str,
    record_key: &str,
    fields: &std::collections::BTreeMap<String, CfdValue>,
) -> Result<(), DiagnosticSet> {
    let schema_view = CftSchemaView::new(&session.schema);
    if !schema_view.has_type(actual_type) {
        return Err(DiagnosticSet::one(Diagnostic::error(
            "WRITE-INSERT",
            "WRITE",
            format!("unknown insert type `{actual_type}`"),
        )));
    }
    for (name, value) in fields {
        let Some(field_ty) = schema_view.field_type(actual_type, name) else {
            return Err(DiagnosticSet::one(Diagnostic::error(
                "WRITE-INSERT",
                "WRITE",
                format!("unknown field `{name}` on type `{actual_type}`"),
            )));
        };
        write_rules::validate_value_for_insert(
            session,
            actual_type,
            record_key,
            field_ty,
            value,
            "WRITE-SHAPE",
            "WRITE",
        )?;
    }
    Ok(())
}

fn ensure_rename_key_available(
    session: &ProjectSession,
    actual_type: &str,
    new_key: &str,
    current_record: CfdRecordId,
) -> Result<(), DiagnosticSet> {
    write_rules::ensure_record_key_available(
        session,
        actual_type,
        new_key,
        Some(current_record),
        "WRITE-RENAME",
        "WRITE",
    )
}
