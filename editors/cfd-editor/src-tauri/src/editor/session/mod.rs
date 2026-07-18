//! Session state and Tauri-facing handlers.
//!
//! `SessionStore` owns a small population of `EditorSession`s — one per
//! loaded project — and dispatches every editor command through a shared
//! `ProviderRegistry`. Each session is wrapped in its own `RwLock` so reads
//! don't block one another and a write is scoped to a single session.
//!
//! After spec 17, the data flow is:
//! 1. `load_project` opens the project and asks `coflow-runtime` to build a
//!    a mutation-capable runtime session (schema, model, diagnostics, and source/record/file
//!    indexes).
//! 2. `get_*` commands read engine state under a read lock and derive only
//!    the wire DTOs they need.
//! 3. `write_field` / `insert_record` / `delete_record` call into the engine
//!    mutation API. The engine owns validation, writer dispatch, and
//!    post-write rebuild; this layer only wraps the mutation report into
//!    editor DTOs.

mod build;
mod diagnostics;
mod dimension;
mod graph;
mod path;
mod revision;

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::Path as StdPath;
use std::path::PathBuf as StdPathBuf;
use std::sync::{Arc, RwLock};

use coflow_api::ProviderRegistry;
use coflow_data_model::CfdValue;
use coflow_runtime::{
    DefaultMaterialization, MutationFields, MutationOp, MutationRequest, MutationValue,
    ProjectQueries, RecordCoordinate, WriteProjectSession,
};

use crate::editor::convert::{annotation_for_draft_field, record_view_to_row, WireContext};
use crate::editor::settings::{
    read_project_settings, sanitized_column_widths, sanitized_record_groups,
    write_project_settings,
};
use crate::editor::types::{
    CollectionEdit, CreateFieldSource, CreateRecordDraft, CreateRecordFieldDraft,
    CreateRequiredInput, DeleteRecordOutcome, DeletedRecordSnapshot, EditorError,
    EditorProjectSettings, EditorRecordGroup, FileRecords, FileTypeOption, GraphData, GraphQuery,
    InsertRecordOutcome, ProjectSnapshot, RecordColumn, RefTarget, RenameRecordOutcome,
    WriteFieldOutcome,
};

pub use diagnostics::Diagnostics;

use build::{
    build_session, default_provider_registry, diagnostic_messages, session_capabilities_for_file,
    SessionSnapshotParts,
};
use path::strip_unc_prefix;
use revision::{RevisionCoordinator, RevisionTicket};

/// A loaded project. Held inside `Arc<RwLock<…>>` so multi-session and
/// multi-reader access stay independent.
pub struct EditorSession {
    pub project_root: std::path::PathBuf,
    /// Path to the project's `coflow.yaml`. Kept for diagnostic / future
    /// API use; rebuilds now happen in place via the engine's write methods
    /// rather than by re-opening the project file.
    #[allow(dead_code)]
    pub yaml_path: std::path::PathBuf,
    pub engine: WriteProjectSession,
    pub diagnostics: Diagnostics,
    file_type_names: BTreeMap<String, Vec<String>>,
    type_display_names: BTreeMap<(String, String), String>,
    ref_target_cache: HashMap<String, Vec<RefTarget>>,
    revisions: RevisionCoordinator,
}

impl std::fmt::Debug for EditorSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EditorSession")
            .field("project_root", &self.project_root)
            .field("source_files", &self.queries().source_file_count())
            .field("records", &self.queries().record_count())
            .finish_non_exhaustive()
    }
}

impl EditorSession {
    const fn queries(&self) -> ProjectQueries<'_> {
        self.engine.queries()
    }

    fn commit_internal_write(&mut self, paths: &[String]) {
        self.revisions
            .commit_internal_write(&self.project_root, paths);
    }

    fn type_display_name(&self, file_path: &str, type_name: &str) -> String {
        self.type_display_names
            .get(&(file_path.to_string(), type_name.to_string()))
            .cloned()
            .unwrap_or_else(|| type_name.to_string())
    }
}

#[derive(Debug)]
struct SessionEntry {
    state: RwLock<EditorSession>,
}

struct ReloadCandidate {
    base_revision: RevisionTicket,
    session: EditorSession,
    snapshot: SessionSnapshotParts,
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
        let outcome = coflow_project::init_project(dir)
            .map_err(|err| EditorError::project(diagnostic_messages(&err)))?;
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
        let snapshot = project_snapshot(id, &session, snapshot_partial);
        inner.sessions.insert(
            id,
            Arc::new(SessionEntry {
                state: RwLock::new(session),
            }),
        );
        drop(inner);
        Ok(snapshot)
    }

    pub fn get_project_settings(&self, id: u32) -> Result<EditorProjectSettings, EditorError> {
        let entry = self.session(id)?;
        let session = entry
            .state
            .read()
            .map_err(|_| EditorError::session("session poisoned during settings read"))?;
        read_project_settings(&session.project_root)
    }

    pub fn get_project_dimensions(
        &self,
        id: u32,
    ) -> Result<Vec<coflow_runtime::DimensionInfo>, EditorError> {
        let entry = self.session(id)?;
        let session = entry
            .state
            .read()
            .map_err(|_| EditorError::session("session poisoned during dimension read"))?;
        Ok(session.queries().dimensions())
    }

    pub fn set_table_column_widths(
        &self,
        id: u32,
        file_path: String,
        actual_type: String,
        widths: BTreeMap<String, f64>,
    ) -> Result<EditorProjectSettings, EditorError> {
        let entry = self.session(id)?;
        let session = entry
            .state
            .write()
            .map_err(|_| EditorError::session("session poisoned during settings write"))?;
        let mut settings = read_project_settings(&session.project_root)?;
        settings
            .table_column_widths
            .entry(file_path)
            .or_default()
            .insert(actual_type, sanitized_column_widths(widths));
        write_project_settings(&session.project_root, &settings)?;
        drop(session);
        Ok(settings)
    }

    pub fn set_record_groups(
        &self,
        id: u32,
        file_path: String,
        actual_type: String,
        groups: Vec<EditorRecordGroup>,
    ) -> Result<EditorProjectSettings, EditorError> {
        let entry = self.session(id)?;
        let session = entry
            .state
            .write()
            .map_err(|_| EditorError::session("session poisoned during settings write"))?;
        let mut settings = read_project_settings(&session.project_root)?;
        settings
            .record_groups
            .entry(file_path)
            .or_default()
            .insert(actual_type, sanitized_record_groups(groups));
        write_project_settings(&session.project_root, &settings)?;
        drop(session);
        Ok(settings)
    }

    pub fn check_project(&self, id: u32) -> Result<String, EditorError> {
        let (yaml_path, registry) = self.project_action_context(id)?;
        let project = coflow_project::Project::open_schema_only(Some(&yaml_path))
            .map_err(|diagnostics| project_diagnostics_to_editor_error(&diagnostics))?;
        match coflow::commands::check_project(&project, registry.as_ref())
            .map_err(|diagnostics| project_diagnostics_to_editor_error(&diagnostics))?
        {
            coflow::commands::CommandOutcome::Success(_) => Ok("Check passed".to_string()),
            coflow::commands::CommandOutcome::Diagnostics(diagnostics) => {
                Err(project_diagnostics_to_editor_error(&diagnostics))
            }
        }
    }

    pub fn build_project(&self, id: u32) -> Result<String, EditorError> {
        let (yaml_path, registry) = self.project_action_context(id)?;
        let project = coflow_project::Project::open_schema_only(Some(&yaml_path))
            .map_err(|diagnostics| project_diagnostics_to_editor_error(&diagnostics))?;
        match coflow::commands::build_project(
            &project,
            registry.as_ref(),
            coflow::commands::BuildOptions::default(),
        )
        .map_err(|diagnostics| project_diagnostics_to_editor_error(&diagnostics))?
        {
            coflow::commands::CommandOutcome::Success(report) => {
                let mut outputs = vec![report.data.dir.display().to_string()];
                if let Some(code) = report.code {
                    outputs.push(code.dir.display().to_string());
                }
                Ok(format!("Build completed: {}", outputs.join(", ")))
            }
            coflow::commands::CommandOutcome::Diagnostics(diagnostics) => {
                Err(project_diagnostics_to_editor_error(&diagnostics))
            }
        }
    }

    pub fn source_file_path(&self, id: u32, file_path: &str) -> Result<StdPathBuf, EditorError> {
        let entry = self.session(id)?;
        let session = entry
            .state
            .read()
            .map_err(|_| EditorError::session("session poisoned during source path lookup"))?;
        if !session.queries().has_source_file(file_path) {
            return Err(EditorError::not_found(format!(
                "`{file_path}` is not a source file in the current project"
            )));
        }
        let root = session.project_root.canonicalize().map_err(|error| {
            EditorError::project(format!("failed to resolve project root: {error}"))
        })?;
        let path = session
            .project_root
            .join(file_path)
            .canonicalize()
            .map_err(|error| {
                EditorError::not_found(format!("failed to resolve `{file_path}`: {error}"))
            })?;
        drop(session);
        if !path.starts_with(&root) || !path.is_file() {
            return Err(EditorError::not_found(format!(
                "source file `{file_path}` is outside the project or does not exist"
            )));
        }
        Ok(path)
    }

    fn project_action_context(
        &self,
        id: u32,
    ) -> Result<(StdPathBuf, Arc<ProviderRegistry>), EditorError> {
        let entry = self.session(id)?;
        let yaml_path = entry
            .state
            .read()
            .map_err(|_| EditorError::session("session poisoned during project action"))?
            .yaml_path
            .clone();
        Ok((yaml_path, self.registry()?))
    }

    pub fn reload_session(&self, id: u32) -> Result<ProjectSnapshot, EditorError> {
        loop {
            let (entry, candidate) = self.build_reload_candidate(id)?;
            if let Some(snapshot) = Self::commit_reload_candidate(id, &entry, candidate)? {
                return Ok(snapshot);
            }
        }
    }

    fn build_reload_candidate(
        &self,
        id: u32,
    ) -> Result<(Arc<SessionEntry>, ReloadCandidate), EditorError> {
        let entry = self.session(id)?;
        let (yaml_path, base_revision) = {
            let session = entry
                .state
                .read()
                .map_err(|_| EditorError::session("session poisoned"))?;
            (session.yaml_path.clone(), session.revisions.begin_reload())
        };
        let registry = self.registry()?;
        let (session, snapshot) = build_session(&yaml_path, registry.as_ref())?;
        Ok((
            entry,
            ReloadCandidate {
                base_revision,
                session,
                snapshot,
            },
        ))
    }

    fn commit_reload_candidate(
        id: u32,
        entry: &SessionEntry,
        mut candidate: ReloadCandidate,
    ) -> Result<Option<ProjectSnapshot>, EditorError> {
        let mut state = entry
            .state
            .write()
            .map_err(|_| EditorError::session("session poisoned"))?;
        let Some(revisions) = state.revisions.commit_reload(candidate.base_revision) else {
            return Ok(None);
        };
        candidate.session.revisions = revisions;
        let snapshot = project_snapshot(id, &candidate.session, candidate.snapshot);
        *state = candidate.session;
        drop(state);
        Ok(Some(snapshot))
    }

    pub(crate) fn has_external_file_changes(
        &self,
        id: u32,
        paths: &[std::path::PathBuf],
    ) -> Result<bool, EditorError> {
        let entry = self.session(id)?;
        let session = entry
            .state
            .read()
            .map_err(|_| EditorError::session("session poisoned"))?;
        Ok(session
            .revisions
            .has_external_change(&session.project_root, paths))
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
        let fields_map = boxed.fields;

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
    session: &EditorSession,
    coordinate: &RecordCoordinate,
) -> Option<DeletedRecordSnapshot> {
    session
        .queries()
        .record_view(&coordinate.actual_type, &coordinate.key)
        .map(|view| DeletedRecordSnapshot {
            record: view.record.clone(),
            display_path: view.display_path.to_string(),
        })
}

fn file_records_for_session(session: &EditorSession, file_path: &str) -> FileRecords {
    let queries = session.queries();
    let ctx = WireContext::new(queries, &session.diagnostics);
    let mut records = Vec::new();
    let mut columns = Vec::<(String, ColumnStats)>::new();
    let mut column_index = BTreeMap::<String, usize>::new();
    let mut type_seen = Vec::new();
    let mut type_set = HashSet::new();
    for view in queries.record_views_in_file(file_path) {
        if type_set.insert(view.coordinate.actual_type.clone()) {
            type_seen.push(view.coordinate.actual_type.clone());
        }
        let row = record_view_to_row(&view, &ctx);
        for field in &row.fields {
            let index = column_index.get(&field.name).copied().unwrap_or_else(|| {
                let index = columns.len();
                columns.push((field.name.clone(), ColumnStats::default()));
                column_index.insert(field.name.clone(), index);
                index
            });
            let stats = &mut columns[index].1;
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
    FileRecords {
        revision: session.revisions.current(),
        file_path: file_path.to_string(),
        type_names: type_seen,
        columns,
        records,
        capabilities: session_capabilities_for_file(session, file_path),
    }
}

fn write_field_in_session(
    session: &mut EditorSession,
    coordinate: &RecordCoordinate,
    field_path: &[coflow_data_model::CfdPathSegment],
    new_value: &CfdValue,
) -> Result<WriteFieldOutcome, EditorError> {
    let old_value = session
        .queries()
        .effective_field_write(coordinate, field_path)
        .and_then(|preview| preview.old_value);
    let report = session.engine.apply_mutation(MutationRequest {
        stop_on_write_error: true,
        ops: vec![MutationOp::SetField {
            record: coordinate.clone(),
            file: None,
            path: field_path.to_vec(),
            value: MutationValue::Cfd(new_value.clone()),
        }],
    });
    let report = finalize_mutation(session, report, "write field failed")?;
    let outcome = report
        .applied
        .first()
        .map(|applied| &applied.outcome)
        .ok_or_else(|| EditorError::write("write field did not apply"))?;
    let renamed = outcome
        .renamed
        .as_ref()
        .and_then(|(old, new)| (old == coordinate).then_some(new.clone()));
    let final_coordinate = renamed.as_ref().unwrap_or(coordinate);
    let queries = session.queries();
    let view = queries
        .record_view(&final_coordinate.actual_type, &final_coordinate.key)
        .ok_or_else(|| {
            EditorError::not_found(format!(
                "record `{}.{}` not found after write",
                final_coordinate.actual_type, final_coordinate.key
            ))
        })?;
    let current_value = queries
        .field_value(
            &final_coordinate.actual_type,
            &final_coordinate.key,
            field_path,
        )
        .cloned();
    let ctx = WireContext::new(queries, &session.diagnostics);
    Ok(WriteFieldOutcome {
        revision: session.revisions.current(),
        row: record_view_to_row(&view, &ctx),
        diagnostics: report.diagnostics,
        old_value,
        new_value: current_value,
        affected_files: report.affected_files,
        renamed,
    })
}

fn first_source_file(nodes: &[coflow_runtime::FileTreeNode]) -> Option<String> {
    for node in nodes {
        if let Some(path) = node.first_source_descendant.clone() {
            return Some(path);
        }
    }
    None
}

fn project_snapshot(
    session_id: u32,
    session: &EditorSession,
    snapshot: SessionSnapshotParts,
) -> ProjectSnapshot {
    let file_types = snapshot_file_types(session, &snapshot.file_tree);
    ProjectSnapshot {
        session_id,
        revision: session.revisions.current(),
        project_root: strip_unc_prefix(&session.project_root.display().to_string()),
        first_source_file: first_source_file(&snapshot.file_tree),
        file_tree: snapshot.file_tree,
        file_types,
        diagnostics: session.diagnostics.to_wire(),
    }
}

fn snapshot_file_types(
    session: &EditorSession,
    nodes: &[coflow_runtime::FileTreeNode],
) -> BTreeMap<String, Vec<FileTypeOption>> {
    let mut files = Vec::new();
    collect_source_files(nodes, &mut files);
    files
        .into_iter()
        .map(|file_path| {
            let mut counts = BTreeMap::<String, usize>::new();
            for view in session.queries().record_views_in_file(&file_path) {
                let type_name = view.coordinate.actual_type.clone();
                *counts.entry(type_name).or_default() += 1;
            }
            let options = session
                .file_type_names
                .get(&file_path)
                .cloned()
                .unwrap_or_else(|| counts.keys().cloned().collect())
                .into_iter()
                .map(|name| FileTypeOption {
                    display_name: session.type_display_name(&file_path, &name),
                    record_count: counts.get(&name).copied().unwrap_or_default(),
                    name,
                })
                .collect();
            (file_path, options)
        })
        .collect()
}

fn collect_source_files(nodes: &[coflow_runtime::FileTreeNode], files: &mut Vec<String>) {
    for node in nodes {
        if node.is_dir {
            collect_source_files(&node.children, files);
        } else if node.in_sources {
            files.push(node.path.clone());
        }
    }
}

fn apply_collection_edit(
    value: CfdValue,
    edit: CollectionEdit,
    default_item: Option<CfdValue>,
) -> Result<CfdValue, EditorError> {
    match (value, edit) {
        (CfdValue::Array(mut items), CollectionEdit::ArrayAppend { value }) => {
            let seed = value
                .or_else(|| items.last().cloned().or(default_item))
                .unwrap_or(CfdValue::Null);
            items.push(seed);
            Ok(CfdValue::Array(items))
        }
        (CfdValue::Array(mut items), CollectionEdit::ArrayRemove { index }) => {
            if index >= items.len() {
                return Err(EditorError::write("array index out of range"));
            }
            items.remove(index);
            Ok(CfdValue::Array(items))
        }
        (CfdValue::Array(mut items), CollectionEdit::ArrayMove { from, to }) => {
            if from >= items.len() || to >= items.len() {
                return Err(EditorError::write("array index out of range"));
            }
            if from != to {
                let moved = items.remove(from);
                items.insert(to, moved);
            }
            Ok(CfdValue::Array(items))
        }
        (CfdValue::Dict(mut entries), CollectionEdit::DictInsert { key, value }) => {
            if entries.iter().any(|(entry_key, _)| entry_key == &key) {
                return Err(EditorError::write("dict key already exists"));
            }
            let seed = value
                .or_else(|| {
                    entries
                        .last()
                        .map(|(_, value)| value.clone())
                        .or(default_item)
                })
                .unwrap_or(CfdValue::Null);
            entries.push((key, seed));
            Ok(CfdValue::Dict(entries))
        }
        (CfdValue::Dict(entries), CollectionEdit::DictRemove { key }) => {
            let original_len = entries.len();
            let entries = entries
                .into_iter()
                .filter(|(entry_key, _)| entry_key != &key)
                .collect::<Vec<_>>();
            if entries.len() == original_len {
                return Err(EditorError::write("dict key not found"));
            }
            Ok(CfdValue::Dict(entries))
        }
        _ => Err(EditorError::write(
            "collection edit target is not a collection",
        )),
    }
}

fn create_record_draft_to_wire(
    draft: &coflow_runtime::CreateRecordDraft,
    ctx: &WireContext<'_>,
) -> CreateRecordDraft {
    CreateRecordDraft {
        actual_type: draft.actual_type.clone(),
        fields: draft
            .fields
            .iter()
            .map(|field| create_record_field_draft_to_wire(&draft.actual_type, field, ctx))
            .collect(),
    }
}

fn create_record_field_draft_to_wire(
    actual_type: &str,
    field: &coflow_runtime::CreateRecordFieldDraft,
    ctx: &WireContext<'_>,
) -> CreateRecordFieldDraft {
    let annotation = field
        .value
        .as_ref()
        .and_then(|value| annotation_for_draft_field(actual_type, &field.name, value, ctx));
    CreateRecordFieldDraft {
        name: field.name.clone(),
        value: field.value.clone(),
        source: create_field_source_to_wire(field.source),
        required: field.required.as_ref().map(create_required_input_to_wire),
        annotation,
    }
}

const fn create_field_source_to_wire(
    source: coflow_runtime::CreateFieldSource,
) -> CreateFieldSource {
    match source {
        coflow_runtime::CreateFieldSource::SchemaDefault => CreateFieldSource::SchemaDefault,
        coflow_runtime::CreateFieldSource::TypeSeed => CreateFieldSource::TypeSeed,
        coflow_runtime::CreateFieldSource::RequiredInput => CreateFieldSource::RequiredInput,
    }
}

fn create_required_input_to_wire(
    input: &coflow_runtime::CreateRequiredInput,
) -> CreateRequiredInput {
    match input {
        coflow_runtime::CreateRequiredInput::Ref { target_type } => CreateRequiredInput::Ref {
            target_type: target_type.clone(),
        },
        coflow_runtime::CreateRequiredInput::AbstractObject {
            expected_type,
            concrete_types,
        } => CreateRequiredInput::AbstractObject {
            expected_type: expected_type.clone(),
            concrete_types: concrete_types.clone(),
        },
        coflow_runtime::CreateRequiredInput::RecursiveObject { type_name } => {
            CreateRequiredInput::RecursiveObject {
                type_name: type_name.clone(),
            }
        }
        coflow_runtime::CreateRequiredInput::Unsupported { message } => {
            CreateRequiredInput::Unsupported {
                message: message.clone(),
            }
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

fn project_diagnostics_to_editor_error(diagnostics: &coflow_api::DiagnosticSet) -> EditorError {
    let message = diagnostics
        .iter()
        .map(|diagnostic| diagnostic.message.as_str())
        .collect::<Vec<_>>()
        .join("; ");
    let flat = diagnostics
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.flat_view(None, None, None))
        .collect();
    EditorError::project(message).with_diagnostics(flat)
}

fn finalize_mutation(
    session: &mut EditorSession,
    report: coflow_runtime::MutationReport,
    fallback: &str,
) -> Result<coflow_runtime::MutationReport, EditorError> {
    if !report.write_ok {
        return Err(mutation_report_to_editor_error(fallback, &report));
    }
    session.diagnostics =
        Diagnostics::from_store(session.queries().diagnostics(), &session.project_root);
    if report.generation_changed {
        session.commit_internal_write(&report.affected_files);
        session.ref_target_cache.clear();
    }
    Ok(report)
}

fn mutation_report_to_editor_error(
    fallback: &str,
    report: &coflow_runtime::MutationReport,
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

#[cfg(test)]
mod revision_tests {
    #![allow(clippy::expect_used, clippy::panic)]

    use std::sync::{mpsc, Arc, Barrier};
    use std::time::Duration;

    use coflow_cft::{DimensionName, FieldName, RecordKey, TypeName, VariantName};
    use coflow_data_model::{CfdPathSegment, CfdValue};
    use coflow_runtime::{DimensionValueCoordinate, DimensionValueState, RecordCoordinate};
    use notify::{Config, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
    use rust_xlsxwriter::Workbook;

    use super::SessionStore;
    use crate::watcher::filter_relevant_paths;

    #[test]
    fn stale_reload_candidate_cannot_replace_a_newer_internal_write() {
        let root = temp_project_dir("stale-reload");
        write_project(&root, "Initial");
        let store = Arc::new(SessionStore::new().expect("create session store"));
        let snapshot = store
            .load_project(&root.join("coflow.yaml"))
            .expect("load project");

        write_project(&root, "External candidate");
        let candidate_built = Arc::new(Barrier::new(2));
        let allow_commit = Arc::new(Barrier::new(2));
        let reload_store = Arc::clone(&store);
        let reload_built = Arc::clone(&candidate_built);
        let reload_commit = Arc::clone(&allow_commit);
        let session_id = snapshot.session_id;
        let reload = std::thread::spawn(move || {
            let (entry, candidate) = reload_store
                .build_reload_candidate(session_id)
                .expect("build reload candidate");
            reload_built.wait();
            reload_commit.wait();
            SessionStore::commit_reload_candidate(session_id, &entry, candidate)
                .expect("attempt candidate commit")
                .is_none()
        });

        candidate_built.wait();
        store
            .write_field(
                session_id,
                &RecordCoordinate::new("Item", "sword"),
                &[CfdPathSegment::Field("name".to_string())],
                &CfdValue::String("Internal write".to_string()),
            )
            .expect("commit internal write");
        allow_commit.wait();

        assert!(reload.join().expect("join reload thread"));
        let records = store
            .get_file_records(session_id, "data/items.cfd")
            .expect("read current session");
        assert_eq!(
            records.records[0].fields[0].value,
            CfdValue::String("Internal write".to_string())
        );
        std::fs::remove_dir_all(root).expect("remove temp project");
    }

    #[test]
    fn dimension_writes_use_authoritative_expected_state() {
        let root = temp_project_dir("dimension-write");
        write_dimension_project(&root);
        let store = SessionStore::new().expect("create session store");
        let snapshot = store
            .load_project(&root.join("coflow.yaml"))
            .expect("load project");
        let coordinate = DimensionValueCoordinate {
            actual_type: TypeName::new("Item").expect("type name"),
            record_key: RecordKey::new("potion").expect("record key"),
            field: FieldName::new("name").expect("field name"),
            dimension: DimensionName::new("language").expect("dimension name"),
            variant: VariantName::new("zh").expect("variant name"),
            path: Vec::new(),
        };
        let initial = DimensionValueState::Value(CfdValue::String("药水".to_string()));
        assert_eq!(
            store
                .get_dimension_value(snapshot.session_id, &coordinate)
                .expect("read dimension value")
                .state,
            initial
        );

        let updated = DimensionValueState::Value(CfdValue::String("治疗药水".to_string()));
        let outcome = store
            .write_dimension_value(snapshot.session_id, &coordinate, &initial, &updated)
            .expect("write dimension value");
        assert_eq!(outcome.old_value, initial);
        assert_eq!(outcome.new_value, updated);

        let stale = store
            .write_dimension_value(
                snapshot.session_id,
                &coordinate,
                &DimensionValueState::Missing,
                &DimensionValueState::Value(CfdValue::String("stale".to_string())),
            )
            .expect_err("stale expected state must fail");
        assert!(stale
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "MUTATION-DIMENSION-STALE"));

        let cleared = store
            .write_dimension_value(
                snapshot.session_id,
                &coordinate,
                &outcome.new_value,
                &DimensionValueState::Missing,
            )
            .expect("clear dimension value");
        assert_eq!(cleared.new_value, DimensionValueState::Missing);
        std::fs::remove_dir_all(root).expect("remove temp project");
    }

    #[test]
    fn file_events_only_match_the_exact_committed_internal_content() {
        let root = temp_project_dir("event-attribution");
        write_project(&root, "Initial");
        let store = SessionStore::new().expect("create session store");
        let snapshot = store
            .load_project(&root.join("coflow.yaml"))
            .expect("load project");
        let source = root.join("data/items.cfd");

        store
            .write_field(
                snapshot.session_id,
                &RecordCoordinate::new("Item", "sword"),
                &[CfdPathSegment::Field("name".to_string())],
                &CfdValue::String("Internal".to_string()),
            )
            .expect("commit internal write");
        assert!(!store
            .has_external_file_changes(snapshot.session_id, std::slice::from_ref(&source))
            .expect("classify internal event"));

        std::fs::write(&source, "sword: Item { name: \"External\" }")
            .expect("write external content");
        assert!(store
            .has_external_file_changes(snapshot.session_id, std::slice::from_ref(&source))
            .expect("classify external event"));
        std::fs::remove_dir_all(root).expect("remove temp project");
    }

    #[test]
    fn watcher_event_batch_for_internal_write_is_not_external() {
        let root = temp_project_dir("watcher-event-attribution");
        write_project(&root, "Initial");
        let store = SessionStore::new().expect("create session store");
        let snapshot = store
            .load_project(&root.join("coflow.yaml"))
            .expect("load project");
        let paths = observed_watcher_paths(&root, || {
            store
                .write_field(
                    snapshot.session_id,
                    &RecordCoordinate::new("Item", "sword"),
                    &[CfdPathSegment::Field("name".to_string())],
                    &CfdValue::String("Internal".to_string()),
                )
                .expect("commit internal write");
        });

        assert!(
            !paths.is_empty(),
            "watcher did not observe the internal write"
        );
        assert!(
            !store
                .has_external_file_changes(snapshot.session_id, &paths)
                .expect("classify watcher batch"),
            "internal watcher batch was classified as external: {paths:?}",
        );
        std::fs::remove_dir_all(root).expect("remove temp project");
    }

    #[test]
    fn excel_watcher_event_batch_for_internal_write_is_not_external() {
        let root = temp_project_dir("excel-watcher-event-attribution");
        write_excel_project(&root);
        let store = SessionStore::new().expect("create session store");
        let snapshot = store
            .load_project(&root.join("coflow.yaml"))
            .expect("load project");
        let paths = observed_watcher_paths(&root, || {
            store
                .write_field(
                    snapshot.session_id,
                    &RecordCoordinate::new("Item", "sword"),
                    &[CfdPathSegment::Field("name".to_string())],
                    &CfdValue::String("Internal".to_string()),
                )
                .expect("commit internal Excel write");
        });

        assert!(
            !paths.is_empty(),
            "watcher did not observe the internal Excel write"
        );
        let relevant_paths = filter_relevant_paths(&paths);
        assert!(
            !store
                .has_external_file_changes(snapshot.session_id, &relevant_paths)
                .expect("classify Excel watcher batch"),
            "internal Excel watcher batch was classified as external: {paths:?}",
        );
        std::fs::remove_dir_all(root).expect("remove temp project");
    }

    fn observed_watcher_paths(
        root: &std::path::Path,
        operation: impl FnOnce(),
    ) -> Vec<std::path::PathBuf> {
        let (sender, receiver) = mpsc::channel();
        let mut watcher = RecommendedWatcher::new(
            move |result| sender.send(result).expect("send watcher event"),
            Config::default(),
        )
        .expect("create watcher");
        watcher
            .watch(root, RecursiveMode::Recursive)
            .expect("watch project");
        operation();

        let mut paths = Vec::new();
        loop {
            match receiver.recv_timeout(Duration::from_millis(500)) {
                Ok(Ok(event)) if !matches!(event.kind, EventKind::Access(_)) => {
                    paths.extend(event.paths);
                }
                Ok(Ok(_)) => {}
                Ok(Err(error)) => panic!("watcher error: {error}"),
                Err(mpsc::RecvTimeoutError::Timeout) => break,
                Err(mpsc::RecvTimeoutError::Disconnected) => panic!("watcher disconnected"),
            }
        }
        drop(watcher);
        paths
    }

    fn write_excel_project(root: &std::path::Path) {
        std::fs::create_dir_all(root.join("data")).expect("create data directory");
        std::fs::write(root.join("schema.cft"), "type Item { name: string; }")
            .expect("write schema");
        std::fs::write(
            root.join("coflow.yaml"),
            "schema: schema.cft\nsources:\n  - path: data/items.xlsx\n    type: excel\n    sheets:\n      - sheet: Item\n        type: Item\n        columns:\n          ID: id\n          Name: name\n",
        )
        .expect("write project configuration");
        let mut workbook = Workbook::new();
        let sheet = workbook.add_worksheet();
        sheet.set_name("Item").expect("name worksheet");
        sheet.write_string(0, 0, "ID").expect("write ID header");
        sheet.write_string(0, 1, "Name").expect("write name header");
        sheet.write_string(1, 0, "sword").expect("write record ID");
        sheet
            .write_string(1, 1, "Sword")
            .expect("write record name");
        workbook
            .save(root.join("data/items.xlsx"))
            .expect("write workbook");
    }

    fn write_project(root: &std::path::Path, name: &str) {
        std::fs::create_dir_all(root.join("data")).expect("create data directory");
        std::fs::write(root.join("schema.cft"), "type Item { name: string; }")
            .expect("write schema");
        std::fs::write(
            root.join("data/items.cfd"),
            format!("sword: Item {{ name: \"{name}\" }}"),
        )
        .expect("write source");
        std::fs::write(
            root.join("coflow.yaml"),
            "schema: schema.cft\nsources:\n  - path: data\n",
        )
        .expect("write project configuration");
    }

    fn write_dimension_project(root: &std::path::Path) {
        std::fs::create_dir_all(root.join("data/dimensions/language"))
            .expect("create dimension directory");
        std::fs::write(
            root.join("schema.cft"),
            "type Item { @localized name: string; }",
        )
        .expect("write schema");
        std::fs::write(root.join("data/items.csv"), "id,name\npotion,Potion\n")
            .expect("write records");
        std::fs::write(
            root.join("data/dimensions/language/Item_name.csv"),
            "id,default,zh\npotion,Potion,药水\n",
        )
        .expect("write dimension values");
        std::fs::write(
            root.join("coflow.yaml"),
            "schema: schema.cft\nsources:\n  - path: data/items.csv\n    type: csv\n    sheets:\n      - sheet: items\n        type: Item\ndimensions:\n  language:\n    variants: [zh]\n    out_dir: data/dimensions/language\n",
        )
        .expect("write project configuration");
    }

    #[test]
    fn project_snapshot_uses_unique_sheet_mapping_as_type_display_name() {
        let root = temp_project_dir("type-display-name");
        write_dimension_project(&root);
        let store = SessionStore::new().expect("create session store");
        let snapshot = store
            .load_project(&root.join("coflow.yaml"))
            .expect("load project");
        let option = snapshot
            .file_types
            .get("data/items.csv")
            .and_then(|options| options.first())
            .expect("file type option");
        assert_eq!(option.name, "Item");
        assert_eq!(option.display_name, "items");
        assert_eq!(option.record_count, 1);
        std::fs::remove_dir_all(root).expect("remove temp project");
    }

    fn temp_project_dir(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "coflow-editor-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        ))
    }
}
