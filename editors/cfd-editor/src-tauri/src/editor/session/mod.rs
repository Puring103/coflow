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
//! 3. `write_field` / `insert_record` / `delete_record` call into the engine
//!    mutation API. The engine owns validation, writer dispatch, and
//!    post-write rebuild; this layer only wraps the mutation report into
//!    editor DTOs.

mod build;
mod diagnostics;
mod graph;
mod path;

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::Path as StdPath;
use std::sync::{Arc, RwLock};

use coflow_api::ProviderRegistry;
use coflow_data_model::CfdValue;
use coflow_engine::{
    DefaultMaterialization, MutationFields, MutationOp, MutationRequest, MutationValue,
    ProjectSession, RecordCoordinate,
};

use crate::editor::convert::{record_view_to_row, WireContext};
use crate::editor::types::{
    DeleteRecordOutcome, DeletedRecordSnapshot, EditorError, FileRecords, GraphData, GraphQuery,
    InsertRecordOutcome, ProjectSnapshot, RecordColumn, RefTarget, RenameRecordOutcome,
    WriteFieldOutcome,
};

pub use diagnostics::Diagnostics;

use build::{build_session, default_provider_registry, session_capabilities_for_file};
use path::strip_unc_prefix;

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
    ref_target_cache: HashMap<String, Vec<RefTarget>>,
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

#[derive(Default)]
struct ColumnStats {
    type_names: BTreeSet<String>,
    max_summary_len: usize,
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
        let mut columns = BTreeMap::<String, ColumnStats>::new();
        let mut type_seen = Vec::new();
        let mut type_set = HashSet::new();
        for view in session.engine.record_views_in_file(file_path) {
            if type_set.insert(view.coordinate.actual_type.clone()) {
                type_seen.push(view.coordinate.actual_type.clone());
            }
            let row = record_view_to_row(&view, &ctx);
            for field in &row.fields {
                let stats = columns.entry(field.name.clone()).or_default();
                stats.type_names.insert(row.coordinate.actual_type.clone());
                let summary_len = row.field_summaries.get(&field.name).map_or(0, String::len);
                stats.max_summary_len = stats.max_summary_len.max(summary_len);
            }
            records.push(row);
        }
        let columns = columns
            .into_iter()
            .map(|(name, stats)| RecordColumn {
                name,
                type_names: stats.type_names.into_iter().collect(),
                max_summary_len: stats.max_summary_len,
            })
            .collect();
        let capabilities =
            session_capabilities_for_file(&session, self.registry()?.as_ref(), file_path);
        drop(session);
        Ok(FileRecords {
            file_path: file_path.to_string(),
            type_names: type_seen,
            columns,
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
        session
            .engine
            .default_record_value(type_name, DefaultMaterialization::EditableShape)
            .map_err(api_diagnostics_to_editor_error)
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
        let targets = {
            let mut session = session_lock
                .write()
                .map_err(|_| EditorError::session("session poisoned"))?;
            if let Some(cached) = session.ref_target_cache.get(expected_type) {
                return Ok(cached.clone());
            }
            let targets = build_ref_targets(&session, expected_type);
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

        let (final_coordinate, renamed) = {
            let mut session = session_lock
                .write()
                .map_err(|_| EditorError::session("session poisoned"))?;
            let report = session
                .engine
                .apply_mutation(
                    registry.as_ref(),
                    MutationRequest {
                        check_after_write: true,
                        stop_on_write_error: true,
                        ops: vec![MutationOp::SetField {
                            record: coordinate.clone(),
                            file: None,
                            path: field_path.to_vec(),
                            value: MutationValue::Cfd(new_value.clone()),
                        }],
                    },
                )
                .map_err(api_diagnostics_to_editor_error)?;
            if !report.write_ok {
                return Err(mutation_report_to_editor_error(
                    "write field failed",
                    &report,
                ));
            }
            let outcome = report
                .applied
                .first()
                .map(|applied| applied.outcome.clone())
                .ok_or_else(|| EditorError::write("write field did not apply"))?;
            session.diagnostics = Diagnostics::from_store(&session.engine.diagnostics);
            let renamed = outcome
                .renamed
                .and_then(|(old, new)| (old == *coordinate).then_some(new));
            let final_coord = renamed.clone().unwrap_or_else(|| coordinate.clone());
            session.ref_target_cache.clear();
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
            let report = session
                .engine
                .apply_mutation(
                    registry.as_ref(),
                    MutationRequest {
                        check_after_write: true,
                        stop_on_write_error: true,
                        ops: vec![MutationOp::InsertRecord {
                            file: file_path.to_string(),
                            sheet: None,
                            actual_type: actual_type.to_string(),
                            key: record_key.to_string(),
                            fields: MutationFields::Cfd(fields_map),
                            materialization,
                        }],
                    },
                )
                .map_err(api_diagnostics_to_editor_error)?;
            if !report.write_ok {
                return Err(mutation_report_to_editor_error(
                    "insert record failed",
                    &report,
                ));
            }
            session.diagnostics = Diagnostics::from_store(&session.engine.diagnostics);
            session.ref_target_cache.clear();
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
            let report = session
                .engine
                .apply_mutation(
                    registry.as_ref(),
                    MutationRequest {
                        check_after_write: true,
                        stop_on_write_error: true,
                        ops: vec![MutationOp::RenameRecord {
                            record: coordinate.clone(),
                            file: None,
                            new_key: new_key.to_string(),
                        }],
                    },
                )
                .map_err(api_diagnostics_to_editor_error)?;
            if !report.write_ok {
                return Err(mutation_report_to_editor_error(
                    "rename record failed",
                    &report,
                ));
            }
            let outcome = report
                .applied
                .first()
                .map(|applied| applied.outcome.clone())
                .ok_or_else(|| EditorError::write("rename did not apply"))?;
            session.diagnostics = Diagnostics::from_store(&session.engine.diagnostics);
            let renamed = outcome
                .renamed
                .and_then(|(old, new)| (old == *coordinate).then_some(new))
                .ok_or_else(|| EditorError::write("rename did not produce a new coordinate"))?;
            session.ref_target_cache.clear();
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
            let report = session
                .engine
                .apply_mutation(
                    registry.as_ref(),
                    MutationRequest {
                        check_after_write: true,
                        stop_on_write_error: true,
                        ops: vec![MutationOp::DeleteRecord {
                            record: coordinate.clone(),
                            file: None,
                        }],
                    },
                )
                .map_err(api_diagnostics_to_editor_error)?;
            if !report.write_ok {
                return Err(mutation_report_to_editor_error(
                    "delete record failed",
                    &report,
                ));
            }
            session.diagnostics = Diagnostics::from_store(&session.engine.diagnostics);
            session.ref_target_cache.clear();
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

fn build_ref_targets(session: &EditorSession, expected_type: &str) -> Vec<RefTarget> {
    let mut targets = Vec::new();
    let Some(domain_id) = session.engine.model.type_domain_id(expected_type) else {
        return targets;
    };
    let Some(members) = session.engine.model.domain_members(domain_id) else {
        return targets;
    };
    for type_id in members {
        let Some(type_name) = session.engine.model.type_name(*type_id) else {
            continue;
        };
        if !session
            .engine
            .schema
            .is_assignable(type_name, expected_type)
        {
            continue;
        }
        for (_, record) in session.engine.model.records_of_type(type_name) {
            let Some(file_path) = session
                .engine
                .file_for_record(record.actual_type(), &record.key)
            else {
                continue;
            };
            targets.push(RefTarget {
                coordinate: RecordCoordinate::new(record.actual_type(), record.key.clone()),
                file_path: file_path.to_string(),
            });
        }
    }
    targets.sort_by(|a, b| {
        a.coordinate
            .actual_type
            .cmp(&b.coordinate.actual_type)
            .then_with(|| a.coordinate.key.cmp(&b.coordinate.key))
    });
    targets.dedup_by(|a, b| a.coordinate == b.coordinate);
    targets
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

fn mutation_report_to_editor_error(
    fallback: &str,
    report: &coflow_engine::MutationReport,
) -> EditorError {
    let message = report
        .failed
        .iter()
        .flat_map(|failed| failed.diagnostics.iter())
        .map(|diagnostic| diagnostic.message.as_str())
        .collect::<Vec<_>>()
        .join("; ");
    let diagnostics = report
        .failed
        .iter()
        .flat_map(|failed| failed.diagnostics.iter().cloned())
        .chain(report.diagnostics.iter().cloned())
        .collect();
    EditorError::write(if message.is_empty() {
        fallback.to_string()
    } else {
        message
    })
    .with_diagnostics(diagnostics)
}
