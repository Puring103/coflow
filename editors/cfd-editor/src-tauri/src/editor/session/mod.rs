//! Session state and Tauri-facing handlers.
//!
//! `SessionStore` owns a small population of `EditorSession`s — one per
//! loaded project — and dispatches every editor command through a shared
//! `ProviderRegistry`. Each session is wrapped in its own `RwLock` so reads
//! don't block one another and a write is scoped to a single session.
//!
//! The data flow is:
//! 1. `load_project` opens the project and asks `coflow-engine` to build a
//!    `ProjectSession` containing schema, model, diagnostics, dependency graph,
//!    and source/record/file indexes.
//! 2. `get_*` commands read engine state under a read lock and derive only the
//!    editor wire DTOs they need.
//! 3. `write_field` uses the engine indexes to find the source and record
//!    origin, routes the edit through the registered `DataWriter`, then rebuilds
//!    the `ProjectSession` from disk before returning a fresh row.
//!
//! The implementation is split across several sub-modules so each one stays
//! short and self-contained:
//! - `build` — open the project and build the shared engine session.
//! - `diagnostics` — convert canonical diagnostics into the wire shape.
//! - `file_tree` — present sources + extension-matched files as a tree.
//! - `graph` — build the BFS-bounded reference graph for a focus file.
//! - `wire` — translate between editor wire types and runtime values.
//! - `path` — small helpers for slash-normalised path strings.
mod build;
mod diagnostics;
mod file_tree;
mod graph;
mod path;
mod wire;

use std::collections::{HashMap, HashSet};
use std::path::Path as StdPath;
use std::sync::{Arc, RwLock};

use coflow_api::{
    DataWriter, ProviderRegistry, RecordOrigin, WriteCellRequest, WriteContext,
    WriteFieldPathSegment,
};
use coflow_data_model::{CfdDataModel, CfdRecord, CfdRecordId};
use coflow_engine::ProjectSession;

use crate::editor::convert::record_to_field_cells_for_session;
use crate::editor::types::{
    EditorError, FieldCell, FieldPathSegment, FieldValue, FileRecords, GraphData, ProjectSnapshot,
    RecordRow, WriteFieldOutcome,
};

pub use diagnostics::Diagnostics;

use build::{build_session, default_provider_registry, session_capabilities_for_file};
use diagnostics::diagnostic_from_api;
use graph::{annotate_ref_files, build_graph};
use path::strip_unc_prefix;
use wire::{default_value_for_ty, field_path_segment_to_api, field_value_to_cfd};

/// A loaded project. Held inside `Arc<RwLock<…>>` so multi-session and
/// multi-reader access stay independent.
pub struct EditorSession {
    pub project_root: std::path::PathBuf,
    pub yaml_path: std::path::PathBuf,
    pub engine: ProjectSession,
    pub diagnostics: Diagnostics,
}

impl std::fmt::Debug for EditorSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EditorSession")
            .field("project_root", &self.project_root)
            .field("source_files", &self.engine.files.source_files().len())
            .field("records", &self.engine.records.by_key().len())
            .finish_non_exhaustive()
    }
}

impl EditorSession {
    fn record_file_map(&self) -> HashMap<String, String> {
        self.engine
            .records
            .by_key()
            .iter()
            .map(|(key, record)| (key.clone(), record.display_path.clone()))
            .collect()
    }
}

#[derive(Debug)]
struct Inner {
    next_id: u32,
    sessions: HashMap<u32, Arc<RwLock<EditorSession>>>,
    registry: Arc<ProviderRegistry>,
}

impl Default for Inner {
    fn default() -> Self {
        Self {
            next_id: 0,
            sessions: HashMap::new(),
            registry: Arc::new(ProviderRegistry::default()),
        }
    }
}

#[derive(Debug)]
pub struct SessionStore {
    inner: RwLock<Inner>,
}

impl SessionStore {
    pub fn new() -> Result<Self, EditorError> {
        Ok(Self {
            inner: RwLock::new(Inner {
                next_id: 0,
                sessions: HashMap::new(),
                registry: Arc::new(default_provider_registry()?),
            }),
        })
    }

    /// Create a minimal new project at `dir` (mirrors the CLI's
    /// `coflow init`) and immediately open it.
    ///
    /// Equivalent to running `coflow init <dir>` and then
    /// `load_project(<dir>/coflow.yaml)` — but in-process, so the front-end
    /// can offer "新建工程" without spawning a subprocess.
    ///
    /// # Errors
    /// Surfaces `EditorErrorKind::Project` when the directory already
    /// holds a `coflow.yaml` or when scaffolding the layout fails.
    pub fn init_project(&self, dir: &StdPath) -> Result<ProjectSnapshot, EditorError> {
        let outcome = coflow_project::init_project(dir).map_err(EditorError::project)?;
        self.load_project(&outcome.config_path)
    }

    /// Open a project: builds a session, returns its id and a snapshot.
    pub fn load_project(&self, yaml_path: &StdPath) -> Result<ProjectSnapshot, EditorError> {
        let registry = self.registry()?;
        let (session, snapshot_partial) = build_session(yaml_path, registry.as_ref())?;
        let mut inner = self
            .inner
            .write()
            .map_err(|_| EditorError::session("session store poisoned"))?;
        inner.next_id = inner.next_id.checked_add(1).unwrap_or(1);
        let id = inner.next_id;
        let project_root = strip_unc_prefix(&session.project_root.display().to_string());
        let diagnostics = session.diagnostics.flatten();
        inner.sessions.insert(id, Arc::new(RwLock::new(session)));
        drop(inner);
        Ok(ProjectSnapshot {
            session_id: id,
            project_root,
            file_tree: snapshot_partial.file_tree,
            diagnostics,
        })
    }

    pub fn close_session(&self, id: u32) -> Result<(), EditorError> {
        self.inner
            .write()
            .map_err(|_| EditorError::session("session store poisoned"))?
            .sessions
            .remove(&id);
        Ok(())
    }

    pub fn get_file_records(&self, id: u32, file_path: &str) -> Result<FileRecords, EditorError> {
        let session_lock = self.session(id)?;
        let session = session_lock
            .read()
            .map_err(|_| EditorError::session("session poisoned"))?;
        let keys = session.engine.records.keys_for_file(file_path).to_vec();

        let mut records = Vec::with_capacity(keys.len());
        let mut type_seen = Vec::new();
        let mut type_set = HashSet::new();
        let record_file_map = session.record_file_map();
        for key in &keys {
            if let Some((_id, record)) = lookup_record_by_key(&session.engine.model, key) {
                if type_set.insert(record.actual_type.clone()) {
                    type_seen.push(record.actual_type.clone());
                }
                let mut row = RecordRow {
                    key: record.key.clone(),
                    actual_type: record.actual_type.clone(),
                    fields: record_to_field_cells_for_session(
                        record,
                        &session.engine.model,
                        &record_file_map,
                    ),
                };
                annotate_ref_files(&mut row.fields, &session);
                records.push(row);
            }
        }
        let capabilities =
            session_capabilities_for_file(&session, self.registry()?.as_ref(), file_path);
        drop(session);
        Ok(FileRecords {
            file_path: file_path.to_string(),
            type_names: type_seen,
            records,
            capabilities,
        })
    }

    pub fn make_default_object(&self, id: u32, type_name: &str) -> Result<FieldValue, EditorError> {
        let session_lock = self.session(id)?;
        let session = session_lock
            .read()
            .map_err(|_| EditorError::session("session poisoned"))?;
        let ty = session
            .engine
            .schema
            .resolve_type(type_name)
            .ok_or_else(|| {
                EditorError::not_found(format!("type `{type_name}` not found in schema"))
            })?;
        let mut fields = Vec::new();
        for f in &ty.all_fields {
            let value = default_value_for_ty(&f.ty_ref, f.default.as_ref(), &session.engine.schema);
            fields.push(FieldCell {
                name: f.name.clone(),
                value,
                is_spread: false,
                spread_info: None,
            });
        }
        drop(session);
        Ok(FieldValue::Object {
            actual_type: type_name.to_string(),
            fields,
        })
    }

    pub fn get_enum_variants(&self, id: u32, enum_name: &str) -> Result<Vec<String>, EditorError> {
        let session_lock = self.session(id)?;
        let session = session_lock
            .read()
            .map_err(|_| EditorError::session("session poisoned"))?;
        Ok(session
            .engine
            .schema
            .resolve_enum(enum_name)
            .map(|e| e.variants.iter().map(|v| v.name.clone()).collect())
            .unwrap_or_default())
    }

    pub fn get_ref_targets(
        &self,
        id: u32,
        expected_type: &str,
    ) -> Result<Vec<String>, EditorError> {
        let session_lock = self.session(id)?;
        let session = session_lock
            .read()
            .map_err(|_| EditorError::session("session poisoned"))?;
        let mut keys: Vec<String> = session
            .engine
            .model
            .records()
            .filter(|(_, r)| {
                session
                    .engine
                    .schema
                    .is_assignable(&r.actual_type, expected_type)
            })
            .map(|(_, r)| r.key.clone())
            .collect();
        drop(session);
        keys.sort();
        keys.dedup();
        Ok(keys)
    }

    pub fn get_graph(&self, id: u32, file_path: &str) -> Result<GraphData, EditorError> {
        let session_lock = self.session(id)?;
        let session = session_lock
            .read()
            .map_err(|_| EditorError::session("session poisoned"))?;
        Ok(build_graph(&session, file_path))
    }

    /// Persist a single field edit and refresh diagnostics.
    ///
    /// Returns a [`WriteFieldOutcome`] containing both the refreshed row
    /// and the project's diagnostics after the post-write rebuild — that
    /// rebuild always reruns the checker, so any check failures the edit
    /// introduced (or fixed) are reflected here without the caller having
    /// to issue a follow-up query.
    pub fn write_field(
        &self,
        id: u32,
        file_path: &str,
        record_key: &str,
        field_path: &[FieldPathSegment],
        new_value: &FieldValue,
    ) -> Result<WriteFieldOutcome, EditorError> {
        let session_lock = self.session(id)?;
        let registry = self.registry()?;
        let path_segments = field_path
            .iter()
            .map(field_path_segment_to_api)
            .collect::<Vec<_>>();

        let yaml_path = write_field_to_source(
            &session_lock,
            registry.as_ref(),
            file_path,
            record_key,
            field_path,
            &path_segments,
            new_value,
        )?;

        // Rebuild the session under a write lock so cross-file ref indexes
        // and check diagnostics stay coherent.
        refresh_session_after_write(&session_lock, registry.as_ref(), &yaml_path)?;

        // Return the refreshed row + diagnostics from the rebuild so the
        // front-end does not need a follow-up diagnostics query.
        let session = session_lock
            .read()
            .map_err(|_| EditorError::session("session poisoned"))?;
        let (_id, record) =
            lookup_record_by_key(&session.engine.model, record_key).ok_or_else(|| {
                EditorError::not_found(format!("record `{record_key}` not found after write"))
            })?;
        let record_file_map = session.record_file_map();
        let mut row = RecordRow {
            key: record.key.clone(),
            actual_type: record.actual_type.clone(),
            fields: record_to_field_cells_for_session(
                record,
                &session.engine.model,
                &record_file_map,
            ),
        };
        annotate_ref_files(&mut row.fields, &session);
        Ok(WriteFieldOutcome {
            row,
            diagnostics: session.diagnostics.flatten(),
        })
    }

    fn registry(&self) -> Result<Arc<ProviderRegistry>, EditorError> {
        let inner = self
            .inner
            .read()
            .map_err(|_| EditorError::session("session store poisoned during registry read"))?;
        Ok(Arc::clone(&inner.registry))
    }

    fn session(&self, id: u32) -> Result<Arc<RwLock<EditorSession>>, EditorError> {
        let inner = self
            .inner
            .read()
            .map_err(|_| EditorError::session("session store poisoned"))?;
        inner
            .sessions
            .get(&id)
            .cloned()
            .ok_or_else(|| EditorError::not_found(format!("unknown session id {id}")))
    }
}

fn write_field_to_source(
    session_lock: &RwLock<EditorSession>,
    registry: &ProviderRegistry,
    file_path: &str,
    record_key: &str,
    field_path: &[FieldPathSegment],
    path_segments: &[WriteFieldPathSegment],
    new_value: &FieldValue,
) -> Result<std::path::PathBuf, EditorError> {
    let session = session_lock
        .read()
        .map_err(|_| EditorError::session("session poisoned"))?;
    let new_cfd_value =
        field_value_to_cfd(new_value, &session.engine.model).map_err(EditorError::write)?;
    let (_id, record) = lookup_record_by_key(&session.engine.model, record_key)
        .ok_or_else(|| EditorError::not_found(format!("record `{record_key}` not found")))?;
    let host_file = session
        .engine
        .records
        .file_for_key(&record.key)
        .map_or_else(|| file_path.to_string(), str::to_string);
    let source = session
        .engine
        .files
        .source_for_display(&host_file)
        .and_then(|source_id| session.engine.sources.entries().get(source_id.index()))
        .map(|entry| entry.source.clone())
        .ok_or_else(|| {
            EditorError::write(format!(
                "no resolved source recorded for file `{host_file}` (cannot dispatch write)"
            ))
        })?;
    let origin = resolve_effective_origin(&session.engine.model, record, field_path);
    let actual_type = record.actual_type.clone();

    let writer: Arc<dyn DataWriter> = registry.writer(&source.provider_id).ok_or_else(|| {
        EditorError::write(format!(
            "no writer registered for provider `{}`",
            source.provider_id
        ))
    })?;

    let write_request = WriteCellRequest {
        origin: &origin,
        record_key: &record.key,
        actual_type: &actual_type,
        field_path: path_segments,
        new_value: &new_cfd_value,
        schema: &session.engine.schema,
        source: &source,
    };
    let write_ctx = WriteContext {
        project_root: &session.project_root,
        schema: &session.engine.schema,
        model: Some(&session.engine.model),
    };
    writer
        .write_field(write_ctx, &write_request)
        .map_err(|diagnostics| {
            let message = diagnostics
                .iter()
                .map(|d| d.message.as_str())
                .collect::<Vec<_>>()
                .join("; ");
            EditorError::write(message).with_diagnostics(
                diagnostics
                    .diagnostics
                    .iter()
                    .map(diagnostic_from_api)
                    .collect(),
            )
        })?;
    Ok(session.yaml_path.clone())
}

fn refresh_session_after_write(
    session_lock: &RwLock<EditorSession>,
    registry: &ProviderRegistry,
    yaml_path: &StdPath,
) -> Result<(), EditorError> {
    let (new_session, _snapshot) = build_session(yaml_path, registry)?;
    let mut session = session_lock
        .write()
        .map_err(|_| EditorError::session("session poisoned"))?;
    *session = new_session;
    drop(session);
    Ok(())
}

fn lookup_record_by_key<'a>(
    model: &'a CfdDataModel,
    key: &str,
) -> Option<(CfdRecordId, &'a CfdRecord)> {
    model.records().find(|(_, record)| record.key == key)
}

/// Resolve the origin to use when writing the given field.
///
/// Always returns the host record's own origin. When the targeted field
/// was inherited via a `...spread`, the writer creates a local override in
/// the host record's source rather than mutating the spread origin —
/// editing `elite_monster.attack` should change the elite monster only,
/// not its `basic_monster` template.
///
/// The `model` and `field_path` arguments are kept on the signature so
/// future overrides (e.g. an explicit "edit at source" gesture from the
/// front-end) can fall back to `record.spread_field_sources[name]`'s
/// origin without changing call sites.
fn resolve_effective_origin(
    model: &CfdDataModel,
    record: &CfdRecord,
    field_path: &[FieldPathSegment],
) -> RecordOrigin {
    let _ = (model, field_path);
    record.origin.clone()
}
