//! Write transaction surface on `ProjectSession`.
//!
//! Hosts call `session.write_field(...)` / `insert_record` / `delete_record`
//! with stable `(actual_type, key)` coordinates. The engine resolves the
//! coordinate to an internal record id, dispatches the edit to the
//! registered writer, then rebuilds itself in place so subsequent queries
//! see the post-write state.

use std::sync::Arc;

use coflow_api::{
    DataWriter, DeleteRecordRequest, Diagnostic, DiagnosticSet, InsertRecordRequest,
    ProviderRegistry, RecordOrigin, ResolvedSource, Severity, WriteCellRequest, WriteContext,
    WriteFieldPathSegment,
};
use coflow_data_model::{CfdRecord, CfdValue};

use super::records::WriteOutcome;
use super::{build_project_session, ProjectSession, RecordCoordinate};

impl ProjectSession {
    /// Persist a single field edit and rebuild the session in place.
    ///
    /// `actual_type` + `key` identify the host record. The writer
    /// preflights before mutating the source — diagnostics from preflight
    /// are returned without rebuilding.
    ///
    /// On success the engine triggers `build_project_session` again to
    /// refresh model, diagnostics, and dependency graph. The
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
        let Some(record_ref) = self.records.get_by_coordinate(actual_type, key) else {
            return Err(DiagnosticSet::one(not_found(actual_type, key)));
        };
        let Some(record) = self.model.record(record_ref.id) else {
            return Err(DiagnosticSet::one(not_found(actual_type, key)));
        };
        let coordinate = record_ref.coordinate.clone();
        let target = write_target_for_path(self, record, record_ref, path)?;
        let source = source_for_file(self, &target.display_path)?;
        let writer = lookup_writer(registry, &source)?;
        let yaml_path = self.project.config_path.clone();

        let write_request = WriteCellRequest {
            origin: &target.origin,
            record_key: &target.coordinate.key,
            actual_type: &target.coordinate.actual_type,
            field_path: path,
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

        let new_session = build_project_session(self.project.clone(), registry).map_err(|err| {
            DiagnosticSet::one(Diagnostic::error(
                "WRITE-REBUILD",
                "WRITE",
                format!("post-write rebuild failed: {err}"),
            ))
        })?;
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
        record_key: &str,
        actual_type: &str,
        fields: &std::collections::BTreeMap<String, CfdValue>,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        let source = source_for_file(self, file)?;
        let sheet = sheet_for_file_type(self, file, actual_type);
        let writer = lookup_writer(registry, &source)?;
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

        let new_session = build_project_session(self.project.clone(), registry).map_err(|err| {
            DiagnosticSet::one(Diagnostic::error(
                "WRITE-REBUILD",
                "WRITE",
                format!("post-write rebuild failed: {err}"),
            ))
        })?;
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
        let writer = lookup_writer(registry, &source)?;
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

        let new_session = build_project_session(self.project.clone(), registry).map_err(|err| {
            DiagnosticSet::one(Diagnostic::error(
                "WRITE-REBUILD",
                "WRITE",
                format!("post-write rebuild failed: {err}"),
            ))
        })?;
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

fn not_found(actual_type: &str, key: &str) -> Diagnostic {
    Diagnostic::error(
        "WRITE-NOT-FOUND",
        "WRITE",
        format!("record `{actual_type}.{key}` was not found in the session"),
    )
}

fn source_for_file(session: &ProjectSession, file: &str) -> Result<ResolvedSource, DiagnosticSet> {
    session
        .files
        .source_for_display(file)
        .and_then(|source_id| session.sources.entries().get(source_id.index()))
        .map(|entry| entry.source.clone())
        .ok_or_else(|| {
            DiagnosticSet::one(Diagnostic::error(
                "WRITE-NO-SOURCE",
                "WRITE",
                format!("no resolved source recorded for file `{file}` (cannot dispatch write)"),
            ))
        })
}

fn lookup_writer(
    registry: &ProviderRegistry,
    source: &ResolvedSource,
) -> Result<Arc<dyn DataWriter>, DiagnosticSet> {
    registry.writer(&source.provider_id).ok_or_else(|| {
        DiagnosticSet::one(Diagnostic {
            code: "WRITE-NO-WRITER".to_string(),
            stage: "WRITE".to_string(),
            severity: Severity::Error,
            message: format!("no writer registered for provider `{}`", source.provider_id),
            primary: None,
            related: Vec::new(),
        })
    })
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

/// Compute the post-write coordinate. Writers don't tell us the new key, so
/// we walk the path: only a write at exactly `[Field("id")]` can rename the
/// record. Everything else preserves the original coordinate.
fn guess_new_coordinate(
    session: &ProjectSession,
    old: &RecordCoordinate,
    path: &[WriteFieldPathSegment],
    new_value: &CfdValue,
) -> RecordCoordinate {
    if path.len() == 1 {
        if let WriteFieldPathSegment::Field(name) = &path[0] {
            if name == "id" {
                if let CfdValue::String(new_key) = new_value {
                    if session
                        .records
                        .get_by_coordinate(&old.actual_type, new_key)
                        .is_some()
                    {
                        return RecordCoordinate::new(&old.actual_type, new_key.clone());
                    }
                }
            }
        }
    }
    let _ = session;
    let _ = (path, new_value);
    old.clone()
}

#[derive(Debug, Clone)]
struct WriteTarget {
    coordinate: RecordCoordinate,
    origin: RecordOrigin,
    display_path: String,
}

fn write_target_for_path(
    session: &ProjectSession,
    host_record: &CfdRecord,
    host_ref: &super::RecordRef,
    path: &[WriteFieldPathSegment],
) -> Result<WriteTarget, DiagnosticSet> {
    let Some(WriteFieldPathSegment::Field(top_field)) = path.first() else {
        return Ok(WriteTarget {
            coordinate: host_ref.coordinate.clone(),
            origin: host_ref.origin.clone(),
            display_path: host_ref.display_path.clone(),
        });
    };
    let Some(source_id) = host_record.spread_source_for_field(top_field) else {
        return Ok(WriteTarget {
            coordinate: host_ref.coordinate.clone(),
            origin: host_ref.origin.clone(),
            display_path: host_ref.display_path.clone(),
        });
    };
    let Some(source_ref) = session.records.get(source_id) else {
        return Err(DiagnosticSet::one(Diagnostic::error(
            "WRITE-SPREAD-SOURCE",
            "WRITE",
            format!("spread source for field `{top_field}` is no longer indexed"),
        )));
    };
    Ok(WriteTarget {
        coordinate: source_ref.coordinate.clone(),
        origin: source_ref.origin.clone(),
        display_path: source_ref.display_path.clone(),
    })
}
