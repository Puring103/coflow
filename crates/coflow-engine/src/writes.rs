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
    ProviderRegistry, RecordOrigin, RenameRecordRequest, ResolvedSource,
    RewriteRecordReferencesRequest, Severity, WriteCellRequest, WriteContext,
    WriteFieldPathSegment,
};
use coflow_data_model::{CfdPath, CfdPathSegment, CfdRecord, CfdRecordId, CfdValue};

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
        ensure_rename_key_available(self, actual_type, new_key)?;

        let target_id = target_ref.id;
        let old_coordinate = target_ref.coordinate.clone();
        let target_origin = target_ref.origin.clone();
        let target_display_path = target_ref.display_path.clone();
        let target_source = source_for_file(self, &target_display_path)?;
        let target_writer = lookup_writer(registry, &target_source)?;
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

        target_writer.rename_record(ctx, &target_request)?;
        for action in &reference_actions {
            let request = action.request.as_request(&self.schema);
            action.writer.write_field(ctx, &request)?;
        }
        for action in &rewrite_actions {
            let request = action.request.as_request(&self.schema);
            action.writer.rewrite_record_references(ctx, &request)?;
        }

        let new_session = build_project_session(self.project.clone(), registry).map_err(|err| {
            DiagnosticSet::one(Diagnostic::error(
                "WRITE-REBUILD",
                "WRITE",
                format!("post-write rebuild failed: {err}"),
            ))
        })?;
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

fn validate_new_record_key(key: &str) -> Result<(), DiagnosticSet> {
    if let Some(reason) = coflow_cft::record_key_ident_error(key) {
        return Err(DiagnosticSet::one(Diagnostic::error(
            "WRITE-RENAME",
            "WRITE",
            format!("record key `{key}` is invalid: {reason}"),
        )));
    }
    Ok(())
}

fn ensure_rename_key_available(
    session: &ProjectSession,
    actual_type: &str,
    new_key: &str,
) -> Result<(), DiagnosticSet> {
    if session
        .records
        .get_by_coordinate(actual_type, new_key)
        .is_some()
    {
        return Err(DiagnosticSet::one(Diagnostic::error(
            "WRITE-RENAME",
            "WRITE",
            format!("record `{actual_type}.{new_key}` already exists"),
        )));
    }
    for target_type in session.schema.assignable_target_names(actual_type) {
        if !session.schema.range_is_polymorphic(&target_type) {
            continue;
        }
        if session
            .model
            .lookup(&target_type, new_key)
            .is_some_and(|id| {
                session.model.record(id).is_some_and(|record| {
                    record.actual_type != actual_type || record.key != new_key
                })
            })
        {
            return Err(DiagnosticSet::one(Diagnostic::error(
                "WRITE-RENAME",
                "WRITE",
                format!("key `{new_key}` already exists in polymorphic range `{target_type}`"),
            )));
        }
    }
    Ok(())
}

struct ReferenceUpdateAction {
    writer: Arc<dyn DataWriter>,
    request: OwnedWriteCellRequest,
}

struct OwnedWriteCellRequest {
    origin: RecordOrigin,
    record_key: String,
    actual_type: String,
    field_path: Vec<WriteFieldPathSegment>,
    new_value: CfdValue,
    source: ResolvedSource,
}

impl OwnedWriteCellRequest {
    fn as_request<'a>(&'a self, schema: &'a coflow_api::CftContainer) -> WriteCellRequest<'a> {
        WriteCellRequest {
            origin: &self.origin,
            record_key: &self.record_key,
            actual_type: &self.actual_type,
            field_path: &self.field_path,
            new_value: &self.new_value,
            schema,
            source: &self.source,
        }
    }
}

struct SourceRewriteAction {
    writer: Arc<dyn DataWriter>,
    request: OwnedRewriteRecordReferencesRequest,
}

struct OwnedRewriteRecordReferencesRequest {
    source: ResolvedSource,
    target_type_names: Vec<String>,
    old_key: String,
    new_key: String,
    rewrite_direct_refs: bool,
}

impl OwnedRewriteRecordReferencesRequest {
    fn as_request<'a>(
        &'a self,
        schema: &'a coflow_api::CftContainer,
    ) -> RewriteRecordReferencesRequest<'a> {
        RewriteRecordReferencesRequest {
            source: &self.source,
            target_type_names: &self.target_type_names,
            old_key: &self.old_key,
            new_key: &self.new_key,
            rewrite_direct_refs: self.rewrite_direct_refs,
            schema,
        }
    }
}

fn reference_update_actions(
    session: &ProjectSession,
    registry: &ProviderRegistry,
    target_id: CfdRecordId,
    new_key: &str,
) -> Result<Vec<ReferenceUpdateAction>, DiagnosticSet> {
    let mut actions = Vec::new();
    for (site, resolved_target) in session.model.ref_sites() {
        if resolved_target != target_id {
            continue;
        }
        let Some(host_ref) = session.records.get(site.host) else {
            continue;
        };
        let Some(host_record) = session.model.record(site.host) else {
            continue;
        };
        let Some(CfdValue::Ref { target_type, .. }) = value_at_path(host_record, &site.path) else {
            continue;
        };
        let source = source_for_file(session, &host_ref.display_path)?;
        let writer = lookup_writer(registry, &source)?;
        actions.push(ReferenceUpdateAction {
            writer,
            request: OwnedWriteCellRequest {
                origin: host_ref.origin.clone(),
                record_key: host_ref.coordinate.key.clone(),
                actual_type: host_ref.coordinate.actual_type.clone(),
                field_path: write_path_from_cfd_path(&site.path)?,
                new_value: CfdValue::Ref {
                    target_type: target_type.clone(),
                    target_key: new_key.to_string(),
                },
                source,
            },
        });
    }
    Ok(actions)
}

fn source_rewrite_actions(
    session: &ProjectSession,
    registry: &ProviderRegistry,
    target_id: CfdRecordId,
    old_key: &str,
    new_key: &str,
) -> Result<Vec<SourceRewriteAction>, DiagnosticSet> {
    let Some(target_record) = session.model.record(target_id) else {
        return Ok(Vec::new());
    };
    let target_type_names = session
        .schema
        .assignable_target_names(&target_record.actual_type);
    let direct_ref_unique = direct_ref_key_is_unique(session, old_key, target_id);
    let mut actions = Vec::new();
    for entry in session.sources.entries() {
        let writer = lookup_writer(registry, &entry.source)?;
        actions.push(SourceRewriteAction {
            writer,
            request: OwnedRewriteRecordReferencesRequest {
                source: entry.source.clone(),
                target_type_names: target_type_names.clone(),
                old_key: old_key.to_string(),
                new_key: new_key.to_string(),
                rewrite_direct_refs: direct_ref_unique,
            },
        });
    }
    Ok(actions)
}

fn direct_ref_key_is_unique(session: &ProjectSession, key: &str, target_id: CfdRecordId) -> bool {
    session
        .model
        .records()
        .all(|(id, record)| record.key != key || id == target_id)
}

fn write_path_from_cfd_path(path: &CfdPath) -> Result<Vec<WriteFieldPathSegment>, DiagnosticSet> {
    path.segments
        .iter()
        .map(|segment| match segment {
            CfdPathSegment::Field(name) => Ok(WriteFieldPathSegment::Field(name.clone())),
            CfdPathSegment::Index(index) => Ok(WriteFieldPathSegment::Index(*index)),
            CfdPathSegment::DictKey(key) => Ok(WriteFieldPathSegment::DictKey(key.clone())),
        })
        .collect()
}

fn value_at_path<'a>(record: &'a CfdRecord, path: &CfdPath) -> Option<&'a CfdValue> {
    let mut segments = path.segments.iter();
    let CfdPathSegment::Field(field) = segments.next()? else {
        return None;
    };
    let mut current = record.fields.get(field)?;
    for segment in segments {
        current = match (segment, current) {
            (CfdPathSegment::Field(field), CfdValue::Object(record)) => record.fields.get(field)?,
            (CfdPathSegment::Index(index), CfdValue::Array(items)) => items.get(*index)?,
            (CfdPathSegment::DictKey(key), CfdValue::Dict(entries)) => entries
                .iter()
                .find(|(entry_key, _)| format_dict_key_for_path(entry_key) == *key)
                .map(|(_, value)| value)?,
            _ => return None,
        };
    }
    Some(current)
}

fn format_dict_key_for_path(key: &coflow_data_model::CfdDictKey) -> String {
    match key {
        coflow_data_model::CfdDictKey::String(value) => format!("\"{value}\""),
        coflow_data_model::CfdDictKey::Int(value) => value.to_string(),
        coflow_data_model::CfdDictKey::Enum(value) => value.variant.as_deref().map_or_else(
            || format!("{}({})", value.enum_name, value.value),
            |variant| format!("{}.{}", value.enum_name, variant),
        ),
    }
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

fn is_id_path(path: &[WriteFieldPathSegment]) -> bool {
    matches!(path, [WriteFieldPathSegment::Field(name)] if name == "id")
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
