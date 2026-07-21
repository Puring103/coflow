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
mod operations;
mod path;
mod revision;

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::Path as StdPath;
use std::path::PathBuf as StdPathBuf;
use std::sync::{Arc, RwLock};

use coflow_api::ProviderRegistry;
use coflow_data_model::{CfdValue, RecordOrigin};
use coflow_runtime::{
    DefaultMaterialization, MutationFields, MutationOp, MutationRequest, MutationValue,
    ProjectQueries, RecordCoordinate, WriteProjectSession,
};

use crate::editor::convert::{annotation_for_draft_field, record_view_to_row, WireContext};
use crate::editor::settings::{
    read_project_settings, sanitized_column_widths, sanitized_record_groups, sanitized_views,
    write_project_settings,
};
use crate::editor::types::{
    BatchWriteFieldInput, BatchWriteFieldEditOutcome, BatchWriteFieldOutcome, CollectionEdit,
    CreateRecordDraft, CreateRecordFieldDraft, DeleteRecordOutcome,
    DeletedRecordSnapshot, EditorError, EditorProjectSettings, FileRecords, FileTypeOption,
    EditorRecordGroup, GraphData, GraphQuery, InsertRecordOutcome, ProjectSnapshot, RecordColumn,
    RefTarget, RenameRecordOutcome, ReorderRecordsOutcome, ViewConfig, WriteFieldOutcome,
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
    /// Path to the project's `coflow.yaml` used by project actions and reloads.
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
        let session = entry.state.read().map_err(|_| EditorError::session("session poisoned"))?;
        Ok(session.queries().dimensions())
    }

    fn project_root_for(&self, id: u32) -> Result<StdPathBuf, EditorError> {
        let entry = self.session(id)?;
        let root = entry
            .state
            .read()
            .map_err(|_| EditorError::session("session poisoned during settings write"))?
            .project_root
            .clone();
        Ok(root)
    }

    /// Set the column widths of the implicit default table view for a
    /// (filePath, actualType).
    pub fn set_default_table_column_widths(
        &self,
        id: u32,
        file_path: String,
        actual_type: String,
        widths: BTreeMap<String, f64>,
    ) -> Result<EditorProjectSettings, EditorError> {
        let project_root = self.project_root_for(id)?;
        let mut settings = read_project_settings(&project_root)?;
        settings
            .default_table_column_widths
            .entry(file_path)
            .or_default()
            .insert(actual_type, sanitized_column_widths(widths));
        write_project_settings(&project_root, &settings)?;
        Ok(settings)
    }

    /// Overwrite the full custom-view list for a (filePath, actualType). The
    /// frontend mutates the list in memory (create/rename/reconfigure/delete)
    /// and submits the whole thing; the backend only sanitizes + persists.
    pub fn set_views(
        &self,
        id: u32,
        file_path: String,
        actual_type: String,
        views: Vec<ViewConfig>,
    ) -> Result<EditorProjectSettings, EditorError> {
        let project_root = self.project_root_for(id)?;
        let mut settings = read_project_settings(&project_root)?;
        let valid_group_ids = settings
            .record_groups
            .get(&file_path)
            .and_then(|by_type| by_type.get(&actual_type))
            .map(|groups| groups.iter().map(|group| group.id.clone()).collect())
            .unwrap_or_default();
        settings
            .views
            .entry(file_path)
            .or_default()
            .insert(actual_type, sanitized_views(views, &valid_group_ids));
        write_project_settings(&project_root, &settings)?;
        Ok(settings)
    }

    /// Update just the `column_widths` of one custom table view in place,
    /// without round-tripping the whole view list (called on drag).
    pub fn set_view_column_widths(
        &self,
        id: u32,
        file_path: String,
        actual_type: String,
        view_id: String,
        widths: BTreeMap<String, f64>,
    ) -> Result<EditorProjectSettings, EditorError> {
        let project_root = self.project_root_for(id)?;
        let mut settings = read_project_settings(&project_root)?;
        if let Some(view) = settings
            .views
            .get_mut(&file_path)
            .and_then(|by_type| by_type.get_mut(&actual_type))
            .and_then(|views| views.iter_mut().find(|view| view.id == view_id))
        {
            view.column_widths = sanitized_column_widths(widths);
            write_project_settings(&project_root, &settings)?;
        }
        Ok(settings)
    }

    pub fn set_record_groups(
        &self,
        id: u32,
        file_path: String,
        actual_type: String,
        groups: Vec<EditorRecordGroup>,
    ) -> Result<EditorProjectSettings, EditorError> {
        let project_root = self.project_root_for(id)?;
        let mut settings = read_project_settings(&project_root)?;
        settings
            .record_groups
            .entry(file_path)
            .or_default()
            .insert(actual_type, sanitized_record_groups(groups));
        write_project_settings(&project_root, &settings)?;
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
                let mut outputs = Vec::new();
                for target in report.targets {
                    outputs.push(target.data.dir.display().to_string());
                    if let Some(code) = target.code {
                        outputs.push(code.dir.display().to_string());
                    }
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
        let project_root = session.project_root.clone();
        drop(session);
        let root = project_root.canonicalize().map_err(|error| {
            EditorError::project(format!("failed to resolve project root: {error}"))
        })?;
        let path = project_root
            .join(file_path)
            .canonicalize()
            .map_err(|error| {
                EditorError::not_found(format!("failed to resolve `{file_path}`: {error}"))
            })?;
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
    let mut container_counts = BTreeMap::<String, usize>::new();
    let mut row_containers = Vec::new();
    for view in queries.record_views_in_file(file_path) {
        if type_set.insert(view.coordinate.actual_type.to_string()) {
            type_seen.push(view.coordinate.actual_type.to_string());
        }
        let container = record_container_key(view.origin);
        let container_index = container_counts.entry(container.clone()).or_default();
        let mut row = record_view_to_row(&view, &ctx);
        row.container_index = *container_index;
        *container_index += 1;
        row_containers.push(container);
        for field in &row.fields {
            let index = column_index.get(&field.name).copied().unwrap_or_else(|| {
                let index = columns.len();
                columns.push((field.name.clone(), ColumnStats::default()));
                column_index.insert(field.name.clone(), index);
                index
            });
            let stats = &mut columns[index].1;
            stats
                .type_names
                .insert(row.coordinate.actual_type.to_string());
            let summary_len = row.field_summaries.get(&field.name).map_or(0, String::len);
            stats.max_summary_len = stats.max_summary_len.max(summary_len);
        }
        records.push(row);
    }
    for (row, container) in records.iter_mut().zip(row_containers) {
        row.container_size = container_counts.get(&container).copied().unwrap_or(1);
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

fn reorder_file_path(
    session: &EditorSession,
    coordinate: &RecordCoordinate,
) -> Result<String, EditorError> {
    session
        .queries()
        .file_for_record(&coordinate.actual_type, &coordinate.key)
        .map(str::to_string)
        .ok_or_else(|| {
            EditorError::not_found(format!(
                "record `{}.{}` not found",
                coordinate.actual_type, coordinate.key
            ))
        })
}

fn record_container_index(session: &EditorSession, coordinate: &RecordCoordinate) -> Option<usize> {
    let file = session
        .queries()
        .file_for_record(&coordinate.actual_type, &coordinate.key)?;
    let views = session
        .queries()
        .record_views_in_file(file)
        .collect::<Vec<_>>();
    let target = views.iter().find(|view| view.coordinate == *coordinate)?;
    let container = record_container_key(target.origin);
    views
        .iter()
        .filter(|view| record_container_key(view.origin) == container)
        .position(|view| view.coordinate == *coordinate)
}

fn record_type_index(session: &EditorSession, coordinate: &RecordCoordinate) -> Option<usize> {
    let file = session
        .queries()
        .file_for_record(&coordinate.actual_type, &coordinate.key)?;
    let views = session
        .queries()
        .record_views_in_file(file)
        .collect::<Vec<_>>();
    let target = views.iter().find(|view| view.coordinate == *coordinate)?;
    let container = record_container_key(target.origin);
    views
        .iter()
        .filter(|view| {
            view.coordinate.actual_type == coordinate.actual_type
                && record_container_key(view.origin) == container
        })
        .position(|view| view.coordinate == *coordinate)
}

fn record_container_key(origin: &RecordOrigin) -> String {
    match origin {
        RecordOrigin::File { path, .. } => format!("file:{}", path.display()),
        RecordOrigin::Table {
            document, sheet, ..
        } => format!("table:{}:{sheet}", document.path().display()),
        RecordOrigin::None => "none".to_string(),
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
    let file_types = snapshot_file_types(session);
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

fn snapshot_file_types(session: &EditorSession) -> BTreeMap<String, Vec<FileTypeOption>> {
    session
        .queries()
        .source_files()
        .map(|file_path| {
            let mut counts = BTreeMap::<String, usize>::new();
            for view in session.queries().record_views_in_file(file_path) {
                let type_name = view.coordinate.actual_type.to_string();
                *counts.entry(type_name).or_default() += 1;
            }
            let options = session
                .file_type_names
                .get(file_path)
                .cloned()
                .unwrap_or_else(|| counts.keys().cloned().collect())
                .into_iter()
                .map(|name| FileTypeOption {
                    display_name: session.type_display_name(file_path, &name),
                    record_count: counts.get(&name).copied().unwrap_or_default(),
                    is_singleton: session.queries().type_is_singleton(&name),
                    name,
                })
                .collect();
            (file_path.to_string(), options)
        })
        .collect()
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
        source: field.source,
        required: field.required.clone(),
        annotation,
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
mod tests;
