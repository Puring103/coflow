//! Session state and Tauri-facing handlers.
//!
//! `SessionStore` owns a small population of `EditorSession`s — one per
//! loaded project — and dispatches every editor command through a shared
//! `ProviderRegistry`. Each session is wrapped in its own `RwLock` so reads
//! don't block one another and a write is scoped to a single session.
//!
//! After spec 17, the data flow is:
//! 1. `load_project` opens the project and asks `coflow-engine` to build a
//!    `ProjectSession` (schema, model, diagnostics, dependency graph, and
//!    source/record/file indexes).
//! 2. `get_*` commands read engine state under a read lock and derive only
//!    the wire DTOs they need.
//! 3. `write_field` / `insert_record` / `delete_record` call straight into
//!    `session.engine.write_field(...)` etc. The engine handles preflight,
//!    writer dispatch, and the post-write rebuild; we just translate the
//!    `(actual_type, key)` coordinate the front-end sent into the engine's
//!    APIs and copy the new diagnostics back.

mod build;
mod diagnostics;
mod graph;
mod path;
mod wire;

use std::collections::{HashMap, HashSet};
use std::path::Path as StdPath;
use std::sync::{Arc, RwLock};

use coflow_api::ProviderRegistry;
use coflow_data_model::{CfdRecord, CfdValue};
use coflow_engine::{ProjectSession, RecordCoordinate};

use crate::editor::convert::{record_view_to_row, WireContext};
use crate::editor::types::{
    DeleteRecordOutcome, DeletedRecordSnapshot, EditorError, FileRecords, GraphData,
    InsertRecordOutcome, ProjectSnapshot, RefTarget, RenameRecordOutcome, WriteFieldOutcome,
};

pub use diagnostics::Diagnostics;

use build::{build_session, default_provider_registry, session_capabilities_for_file};
use path::strip_unc_prefix;
use wire::default_value_for_ty;

/// A loaded project. Held inside `Arc<RwLock<…>>` so multi-session and
/// multi-reader access stay independent.
pub struct EditorSession {
    pub project_root: std::path::PathBuf,
    /// Path to the project's `coflow.yaml`. Kept for diagnostic / future
    /// API use; rebuilds now happen in place via the engine's write methods
    /// rather than by re-opening the project file.
    #[allow(dead_code)]
    pub yaml_path: std::path::PathBuf,
    pub engine: ProjectSession,
    pub diagnostics: Diagnostics,
}

impl std::fmt::Debug for EditorSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EditorSession")
            .field("project_root", &self.project_root)
            .field("source_files", &self.engine.files.source_files().len())
            .field("records", &self.engine.records.by_id().len())
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
struct SessionEntry {
    state: RwLock<EditorSession>,
}

#[derive(Debug)]
struct Inner {
    next_id: u32,
    sessions: HashMap<u32, Arc<SessionEntry>>,
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

    pub fn init_project(&self, dir: &StdPath) -> Result<ProjectSnapshot, EditorError> {
        let outcome = coflow_project::init_project(dir).map_err(EditorError::project)?;
        self.load_project(&outcome.config_path)
    }

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
        inner.sessions.insert(
            id,
            Arc::new(SessionEntry {
                state: RwLock::new(session),
            }),
        );
        drop(inner);
        Ok(ProjectSnapshot {
            session_id: id,
            project_root,
            file_tree: snapshot_partial.file_tree,
            diagnostics,
        })
    }

    pub fn reload_session(&self, id: u32) -> Result<ProjectSnapshot, EditorError> {
        let entry = self.session(id)?;
        let yaml_path = {
            let session = entry
                .state
                .read()
                .map_err(|_| EditorError::session("session poisoned"))?;
            session.yaml_path.clone()
        };
        let registry = self.registry()?;
        let (session, snapshot_partial) = build_session(&yaml_path, registry.as_ref())?;
        let project_root = strip_unc_prefix(&session.project_root.display().to_string());
        let diagnostics = session.diagnostics.flatten();
        {
            let mut state = entry
                .state
                .write()
                .map_err(|_| EditorError::session("session poisoned"))?;
            *state = session;
        }
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
        let entry = self.session(id)?;
        let session_lock = &entry.state;
        let session = session_lock
            .read()
            .map_err(|_| EditorError::session("session poisoned"))?;
        let ctx = WireContext {
            session: &session.engine,
        };
        let mut records = Vec::new();
        let mut type_seen = Vec::new();
        let mut type_set = HashSet::new();
        for view in session.engine.record_views_in_file(file_path) {
            if type_set.insert(view.coordinate.actual_type.clone()) {
                type_seen.push(view.coordinate.actual_type.clone());
            }
            records.push(record_view_to_row(&view, &ctx));
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

    pub fn make_default_object(&self, id: u32, type_name: &str) -> Result<CfdValue, EditorError> {
        let entry = self.session(id)?;
        let session_lock = &entry.state;
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
        let mut fields = std::collections::BTreeMap::new();
        for f in &ty.all_fields {
            let value = default_value_for_ty(&f.ty_ref, f.default.as_ref(), &session.engine.schema);
            fields.insert(f.name.clone(), value);
        }
        drop(session);
        Ok(CfdValue::Object(Box::new(CfdRecord {
            key: String::new(),
            actual_type: type_name.to_string(),
            fields,
            origin: coflow_data_model::RecordOrigin::None,
            spread_field_sources: std::collections::BTreeMap::new(),
        })))
    }

    pub fn get_enum_variants(&self, id: u32, enum_name: &str) -> Result<Vec<String>, EditorError> {
        let entry = self.session(id)?;
        let session_lock = &entry.state;
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

    /// Records assignable to `expected_type`, surfaced as `RefTarget`s so
    /// the front-end can render `Type.key` and jump directly.
    pub fn get_ref_targets(
        &self,
        id: u32,
        expected_type: &str,
    ) -> Result<Vec<RefTarget>, EditorError> {
        let entry = self.session(id)?;
        let session_lock = &entry.state;
        let session = session_lock
            .read()
            .map_err(|_| EditorError::session("session poisoned"))?;
        let mut targets: Vec<RefTarget> = session
            .engine
            .records
            .by_id()
            .values()
            .filter(|record_ref| {
                session
                    .engine
                    .schema
                    .is_assignable(&record_ref.coordinate.actual_type, expected_type)
            })
            .map(|record_ref| RefTarget {
                coordinate: record_ref.coordinate.clone(),
                file_path: record_ref.display_path.clone(),
            })
            .collect();
        drop(session);
        targets.sort_by(|a, b| {
            a.coordinate
                .actual_type
                .cmp(&b.coordinate.actual_type)
                .then_with(|| a.coordinate.key.cmp(&b.coordinate.key))
        });
        targets.dedup_by(|a, b| a.coordinate == b.coordinate);
        Ok(targets)
    }

    pub fn get_graph(&self, id: u32, file_path: &str) -> Result<GraphData, EditorError> {
        let entry = self.session(id)?;
        let session_lock = &entry.state;
        let session = session_lock
            .read()
            .map_err(|_| EditorError::session("session poisoned"))?;
        Ok(graph::build_graph(&session, file_path))
    }

    /// Persist a single field edit. Coordinate carries the host record's
    /// `(actual_type, key)` so the engine can address synthetic-vs-source
    /// rows that share a key.
    pub fn write_field(
        &self,
        id: u32,
        coordinate: &RecordCoordinate,
        field_path: &[coflow_data_model::CfdPathSegment],
        new_value: &CfdValue,
    ) -> Result<WriteFieldOutcome, EditorError> {
        let entry = self.session(id)?;
        let session_lock = &entry.state;
        let registry = self.registry()?;
        let path_segments: Vec<coflow_api::WriteFieldPathSegment> = field_path
            .iter()
            .map(coflow_path_to_write_segment)
            .collect();

        let (final_coordinate, renamed) = {
            let mut session = session_lock
                .write()
                .map_err(|_| EditorError::session("session poisoned"))?;
            let outcome = session
                .engine
                .write_field(
                    registry.as_ref(),
                    &coordinate.actual_type,
                    &coordinate.key,
                    &path_segments,
                    new_value,
                )
                .map_err(api_diagnostics_to_editor_error)?;
            session.diagnostics = Diagnostics::from_store(&session.engine.diagnostics);
            let renamed = outcome
                .renamed
                .and_then(|(old, new)| (old == *coordinate).then_some(new));
            let final_coord = renamed.clone().unwrap_or_else(|| coordinate.clone());
            drop(session);
            (final_coord, renamed)
        };

        let session = session_lock
            .read()
            .map_err(|_| EditorError::session("session poisoned"))?;
        let view = session
            .engine
            .record_view(&final_coordinate.actual_type, &final_coordinate.key)
            .ok_or_else(|| {
                EditorError::not_found(format!(
                    "record `{}.{}` not found after write",
                    final_coordinate.actual_type, final_coordinate.key
                ))
            })?;
        let ctx = WireContext {
            session: &session.engine,
        };
        let row = record_view_to_row(&view, &ctx);
        Ok(WriteFieldOutcome {
            row,
            diagnostics: session.diagnostics.flatten(),
            renamed,
        })
    }

    pub fn insert_record(
        &self,
        id: u32,
        file_path: &str,
        record_key: &str,
        actual_type: &str,
        fields: CfdValue,
    ) -> Result<InsertRecordOutcome, EditorError> {
        let entry = self.session(id)?;
        let session_lock = &entry.state;
        let registry = self.registry()?;
        let CfdValue::Object(boxed) = fields else {
            return Err(EditorError::write(
                "insert_record requires a CfdValue::Object for fields",
            ));
        };
        let fields_map = boxed.fields;

        {
            let mut session = session_lock
                .write()
                .map_err(|_| EditorError::session("session poisoned"))?;
            session
                .engine
                .insert_record(
                    registry.as_ref(),
                    file_path,
                    None,
                    record_key,
                    actual_type,
                    &fields_map,
                )
                .map_err(api_diagnostics_to_editor_error)?;
            session.diagnostics = Diagnostics::from_store(&session.engine.diagnostics);
        }
        let file_records = self.get_file_records(id, file_path)?;
        let session = session_lock
            .read()
            .map_err(|_| EditorError::session("session poisoned"))?;
        Ok(InsertRecordOutcome {
            file_records,
            diagnostics: session.diagnostics.flatten(),
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
        let registry = self.registry()?;
        let renamed = {
            let mut session = session_lock
                .write()
                .map_err(|_| EditorError::session("session poisoned"))?;
            let outcome = session
                .engine
                .rename_record_key(
                    registry.as_ref(),
                    &coordinate.actual_type,
                    &coordinate.key,
                    new_key,
                )
                .map_err(api_diagnostics_to_editor_error)?;
            session.diagnostics = Diagnostics::from_store(&session.engine.diagnostics);
            let renamed = outcome
                .renamed
                .and_then(|(old, new)| (old == *coordinate).then_some(new))
                .ok_or_else(|| EditorError::write("rename did not produce a new coordinate"))?;
            drop(session);
            renamed
        };

        let session = session_lock
            .read()
            .map_err(|_| EditorError::session("session poisoned"))?;
        let view = session
            .engine
            .record_view(&renamed.actual_type, &renamed.key)
            .ok_or_else(|| {
                EditorError::not_found(format!(
                    "record `{}.{}` not found after rename",
                    renamed.actual_type, renamed.key
                ))
            })?;
        let ctx = WireContext {
            session: &session.engine,
        };
        let row = record_view_to_row(&view, &ctx);
        Ok(RenameRecordOutcome {
            row,
            diagnostics: session.diagnostics.flatten(),
            renamed,
        })
    }

    pub fn delete_record(
        &self,
        id: u32,
        coordinate: &RecordCoordinate,
    ) -> Result<DeleteRecordOutcome, EditorError> {
        let entry = self.session(id)?;
        let session_lock = &entry.state;
        let registry = self.registry()?;
        let deleted_snapshot = snapshot_record_before_delete(session_lock, coordinate)?;
        let file_path = deleted_snapshot
            .as_ref()
            .map(|snapshot| snapshot.display_path.clone())
            .or_else(|| {
                let session = session_lock.read().ok()?;
                session
                    .engine
                    .file_for_record(&coordinate.actual_type, &coordinate.key)
                    .map(str::to_string)
            })
            .ok_or_else(|| {
                EditorError::not_found(format!(
                    "record `{}.{}` not found",
                    coordinate.actual_type, coordinate.key
                ))
            })?;
        {
            let mut session = session_lock
                .write()
                .map_err(|_| EditorError::session("session poisoned"))?;
            session
                .engine
                .delete_record(registry.as_ref(), &coordinate.actual_type, &coordinate.key)
                .map_err(api_diagnostics_to_editor_error)?;
            session.diagnostics = Diagnostics::from_store(&session.engine.diagnostics);
        }
        let file_records = self.get_file_records(id, &file_path)?;
        let session = session_lock
            .read()
            .map_err(|_| EditorError::session("session poisoned"))?;
        Ok(DeleteRecordOutcome {
            file_records,
            diagnostics: session.diagnostics.flatten(),
            deleted_snapshot,
        })
    }

    fn registry(&self) -> Result<Arc<ProviderRegistry>, EditorError> {
        let inner = self
            .inner
            .read()
            .map_err(|_| EditorError::session("session store poisoned during registry read"))?;
        Ok(Arc::clone(&inner.registry))
    }

    fn session(&self, id: u32) -> Result<Arc<SessionEntry>, EditorError> {
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

/// Capture the record as `(CfdRecord, display_path)` before the writer
/// touches the source. Returns `None` when no record matches the
/// coordinate — undo is best-effort in that case.
fn snapshot_record_before_delete(
    session_lock: &RwLock<EditorSession>,
    coordinate: &RecordCoordinate,
) -> Result<Option<DeletedRecordSnapshot>, EditorError> {
    let snapshot = {
        let session = session_lock
            .read()
            .map_err(|_| EditorError::session("session poisoned"))?;
        session
            .engine
            .record_view(&coordinate.actual_type, &coordinate.key)
            .map(|view| DeletedRecordSnapshot {
                record: view.record.clone(),
                display_path: view.display_path.to_string(),
            })
    };
    Ok(snapshot)
}

fn coflow_path_to_write_segment(
    segment: &coflow_data_model::CfdPathSegment,
) -> coflow_api::WriteFieldPathSegment {
    match segment {
        coflow_data_model::CfdPathSegment::Field(name) => {
            coflow_api::WriteFieldPathSegment::Field(name.clone())
        }
        coflow_data_model::CfdPathSegment::Index(i) => coflow_api::WriteFieldPathSegment::Index(*i),
        coflow_data_model::CfdPathSegment::DictKey(key) => {
            coflow_api::WriteFieldPathSegment::DictKey(key.clone())
        }
    }
}

#[allow(clippy::needless_pass_by_value)]
fn api_diagnostics_to_editor_error(diagnostics: coflow_api::DiagnosticSet) -> EditorError {
    let message = diagnostics
        .iter()
        .map(|d| d.message.as_str())
        .collect::<Vec<_>>()
        .join("; ");
    let flat: Vec<coflow_api::FlatDiagnostic> = diagnostics
        .diagnostics
        .iter()
        .map(|d| d.flat_view(None, None, None))
        .collect();
    EditorError::write(message).with_diagnostics(flat)
}
