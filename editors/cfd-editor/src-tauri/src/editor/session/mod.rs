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
mod graph;
mod path;
mod wire;

use std::collections::{HashMap, HashSet};
use std::path::Path as StdPath;
use std::sync::{Arc, RwLock};

use coflow_api::ProviderRegistry;
use coflow_data_model::{CfdRecord, CfdRecordId};
use coflow_engine::ProjectSession;

use crate::editor::convert::record_to_field_cells_for_session;
use crate::editor::types::{
    DeleteRecordOutcome, EditorError, FieldCell, FieldPathSegment, FieldValue, FileRecords,
    GraphData, InsertRecordOutcome, ProjectSnapshot, RecordRow, WriteFieldOutcome,
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

impl EditorSession {
    /// `record_key → file_path` map for cells that carry a record reference.
    /// Uses the first record found for a given key (synthetic records of a
    /// given key share the same backing file as their source, so collisions
    /// in this map don't matter for navigation).
    fn record_file_map(&self) -> HashMap<String, String> {
        self.engine
            .records
            .by_id()
            .values()
            .map(|record| (record.coordinate.key.clone(), record.display_path.clone()))
            .collect()
    }
}

/// Container for one open project. Writes acquire the inner `RwLock` for
/// write access — the engine's `write_field` / `insert_record` /
/// `delete_record` mutates the session in place (rebuilding model +
/// diagnostics on success), so the lock alone is enough to serialise
/// concurrent edits.
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
        let ids = session.engine.records.ids_in_file(file_path).to_vec();

        let mut records = Vec::with_capacity(ids.len());
        let mut type_seen = Vec::new();
        let mut type_set = HashSet::new();
        let record_file_map = session.record_file_map();
        for id in &ids {
            if let Some(record) = session.engine.model.record(*id) {
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

    pub fn get_ref_targets(
        &self,
        id: u32,
        expected_type: &str,
    ) -> Result<Vec<String>, EditorError> {
        let entry = self.session(id)?;
        let session_lock = &entry.state;
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
        let entry = self.session(id)?;
        let session_lock = &entry.state;
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
        let entry = self.session(id)?;
        let session_lock = &entry.state;
        let registry = self.registry()?;
        let path_segments = field_path
            .iter()
            .map(field_path_segment_to_api)
            .collect::<Vec<_>>();

        // Convert wire value → core CfdValue + resolve coordinate inside the
        // write lock so the engine sees a coherent snapshot.
        let (new_cfd_value, coordinate) = {
            let session = session_lock
                .read()
                .map_err(|_| EditorError::session("session poisoned"))?;
            let value = field_value_to_cfd(new_value, &session.engine.model)
                .map_err(EditorError::write)?;
            let (_id, record) = lookup_record_in_file(&session.engine, file_path, record_key)
                .ok_or_else(|| {
                    EditorError::not_found(format!(
                        "record `{record_key}` not found in `{file_path}`"
                    ))
                })?;
            (
                value,
                coflow_engine::RecordCoordinate::new(
                    record.actual_type.clone(),
                    record.key.clone(),
                ),
            )
        };

        let new_coordinate = {
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
                    &new_cfd_value,
                )
                .map_err(api_diagnostics_to_editor_error)?;
            // Engine has already replaced `session.engine` with the rebuilt
            // ProjectSession; refresh editor's `Diagnostics` view to match.
            session.diagnostics = Diagnostics::from_set(outcome.diagnostics.clone());
            outcome
                .renamed
                .map(|(_, new)| new)
                .or_else(|| outcome.touched.into_iter().next())
                .unwrap_or(coordinate)
        };

        let session = session_lock
            .read()
            .map_err(|_| EditorError::session("session poisoned"))?;
        let (_id, record) = lookup_record_in_file(
            &session.engine,
            file_path,
            &new_coordinate.key,
        )
        .ok_or_else(|| {
            EditorError::not_found(format!(
                "record `{}` not found in `{file_path}` after write",
                new_coordinate.key
            ))
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

    /// Persist a new top-level record and refresh diagnostics.
    pub fn insert_record(
        &self,
        id: u32,
        file_path: &str,
        record_key: &str,
        actual_type: &str,
        fields: &FieldValue,
    ) -> Result<InsertRecordOutcome, EditorError> {
        let entry = self.session(id)?;
        let session_lock = &entry.state;
        let registry = self.registry()?;

        let fields_map = {
            let session = session_lock
                .read()
                .map_err(|_| EditorError::session("session poisoned"))?;
            let FieldValue::Object { fields: cells, .. } = fields else {
                return Err(EditorError::write(
                    "insert_record requires an Object FieldValue for fields",
                ));
            };
            let mut map = std::collections::BTreeMap::new();
            for cell in cells {
                let value = field_value_to_cfd(&cell.value, &session.engine.model)
                    .map_err(EditorError::write)?;
                map.insert(cell.name.clone(), value);
            }
            map
        };

        {
            let mut session = session_lock
                .write()
                .map_err(|_| EditorError::session("session poisoned"))?;
            let outcome = session
                .engine
                .insert_record(
                    registry.as_ref(),
                    file_path,
                    record_key,
                    actual_type,
                    fields_map,
                )
                .map_err(api_diagnostics_to_editor_error)?;
            session.diagnostics = Diagnostics::from_set(outcome.diagnostics);
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

    /// Delete a top-level record and refresh diagnostics.
    ///
    /// Captures a wire-shaped snapshot of the record **before** the writer
    /// touches the source so the front-end's undo can re-insert it later
    /// without depending on its `fileDataCache`. The snapshot reflects the
    /// engine's authoritative view (spread metadata, ref types, ...) at the
    /// moment of deletion.
    pub fn delete_record(
        &self,
        id: u32,
        file_path: &str,
        record_key: &str,
    ) -> Result<DeleteRecordOutcome, EditorError> {
        let entry = self.session(id)?;
        let session_lock = &entry.state;
        let registry = self.registry()?;
        let (deleted_snapshot, deleted_actual_type) =
            snapshot_record_before_delete(session_lock, file_path, record_key)?;
        let coordinate = {
            let session = session_lock
                .read()
                .map_err(|_| EditorError::session("session poisoned"))?;
            let (_id, record) = lookup_record_in_file(&session.engine, file_path, record_key)
                .ok_or_else(|| {
                    EditorError::not_found(format!(
                        "record `{record_key}` not found in `{file_path}`"
                    ))
                })?;
            coflow_engine::RecordCoordinate::new(record.actual_type.clone(), record.key.clone())
        };
        {
            let mut session = session_lock
                .write()
                .map_err(|_| EditorError::session("session poisoned"))?;
            let outcome = session
                .engine
                .delete_record(
                    registry.as_ref(),
                    &coordinate.actual_type,
                    &coordinate.key,
                )
                .map_err(api_diagnostics_to_editor_error)?;
            session.diagnostics = Diagnostics::from_set(outcome.diagnostics);
        }
        let file_records = self.get_file_records(id, file_path)?;
        let session = session_lock
            .read()
            .map_err(|_| EditorError::session("session poisoned"))?;
        Ok(DeleteRecordOutcome {
            file_records,
            diagnostics: session.diagnostics.flatten(),
            deleted_snapshot,
            deleted_actual_type,
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

/// Render the engine's current view of `record_key` as a wire `FieldValue`
/// suitable for round-tripping back through `insert_record` later (for undo).
///
/// Returns `(None, None)` when the record cannot be found in the model.
/// Callers should treat that as "no snapshot available" rather than an
/// error — deletion of an unknown key will fail at the writer anyway.
fn snapshot_record_before_delete(
    session_lock: &RwLock<EditorSession>,
    file_path: &str,
    record_key: &str,
) -> Result<(Option<FieldValue>, Option<String>), EditorError> {
    let (snapshot, actual_type) = {
        let session = session_lock
            .read()
            .map_err(|_| EditorError::session("session poisoned"))?;
        let Some((_, record)) = lookup_record_in_file(&session.engine, file_path, record_key)
        else {
            return Ok((None, None));
        };
        let record_file_map = session.record_file_map();
        let fields =
            record_to_field_cells_for_session(record, &session.engine.model, &record_file_map);
        let actual_type = record.actual_type.clone();
        drop(session);
        (
            FieldValue::Object {
                actual_type: actual_type.clone(),
                fields,
            },
            actual_type,
        )
    };
    Ok((Some(snapshot), Some(actual_type)))
}

#[allow(clippy::needless_pass_by_value)]
fn api_diagnostics_to_editor_error(diagnostics: coflow_api::DiagnosticSet) -> EditorError {
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
}

/// Locate a record by `(file_path, key)`. Files only host records of a single
/// `actual_type` in practice (the editor presents one type per table), so a
/// `key`-only match scoped to a specific file uniquely identifies the row
/// even when the same key exists in a sibling type (e.g. source `Item.potion`
/// vs synthetic `Item_nameVariants.potion`).
fn lookup_record_in_file<'a>(
    engine: &'a ProjectSession,
    file_path: &str,
    key: &str,
) -> Option<(CfdRecordId, &'a CfdRecord)> {
    for id in engine.records.ids_in_file(file_path) {
        let Some(record) = engine.model.record(*id) else {
            continue;
        };
        if record.key == key {
            return Some((*id, record));
        }
    }
    None
}
