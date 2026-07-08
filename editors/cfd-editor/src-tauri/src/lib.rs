#![allow(clippy::multiple_crate_versions)]

use std::path::PathBuf;

pub mod editor;
mod watcher;

use coflow_data_model::{CfdPathSegment, CfdValue};
use coflow_runtime::RecordCoordinate;
use editor::{
    CollectionEdit, DeleteRecordOutcome, EditorError, FileRecords, GraphData, GraphQuery,
    InsertRecordOutcome, ProjectSnapshot, RefTarget, RenameRecordOutcome, SessionStore,
    WriteFieldOutcome,
};
use tauri::{AppHandle, Manager, State};
use watcher::ProjectWatchRegistry;

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
fn load_project(
    yaml_path: String,
    store: State<'_, SessionStore>,
    watchers: State<'_, ProjectWatchRegistry>,
    app: AppHandle,
) -> Result<ProjectSnapshot, EditorError> {
    let snapshot = store.load_project(&PathBuf::from(yaml_path))?;
    watchers.watch_session(app, &snapshot)?;
    Ok(snapshot)
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
fn init_project(
    dir: String,
    store: State<'_, SessionStore>,
    watchers: State<'_, ProjectWatchRegistry>,
    app: AppHandle,
) -> Result<ProjectSnapshot, EditorError> {
    let snapshot = store.init_project(&PathBuf::from(dir))?;
    watchers.watch_session(app, &snapshot)?;
    Ok(snapshot)
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
fn close_session(
    session_id: u32,
    store: State<'_, SessionStore>,
    watchers: State<'_, ProjectWatchRegistry>,
) -> Result<(), EditorError> {
    watchers.unwatch_session(session_id);
    store.close_session(session_id)
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
fn get_file_records(
    session_id: u32,
    file_path: String,
    store: State<'_, SessionStore>,
) -> Result<FileRecords, EditorError> {
    store.get_file_records(session_id, &file_path)
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
fn get_graph(
    session_id: u32,
    file_path: String,
    active_type: Option<String>,
    enabled_fields: Option<Vec<String>>,
    depth: Option<usize>,
    limit: Option<usize>,
    store: State<'_, SessionStore>,
) -> Result<GraphData, EditorError> {
    store.get_graph(
        session_id,
        &GraphQuery {
            file_path,
            active_type,
            enabled_fields,
            depth,
            limit,
        },
    )
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
fn get_enum_variants(
    session_id: u32,
    enum_name: String,
    store: State<'_, SessionStore>,
) -> Result<Vec<String>, EditorError> {
    store.get_enum_variants(session_id, &enum_name)
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
fn get_ref_targets(
    session_id: u32,
    target_type: String,
    store: State<'_, SessionStore>,
) -> Result<Vec<RefTarget>, EditorError> {
    store.get_ref_targets(session_id, &target_type)
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
fn make_default_object(
    session_id: u32,
    type_name: String,
    store: State<'_, SessionStore>,
) -> Result<CfdValue, EditorError> {
    store.make_default_object(session_id, &type_name)
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
fn write_field(
    session_id: u32,
    coordinate: RecordCoordinate,
    field_path: Vec<CfdPathSegment>,
    new_value: CfdValue,
    store: State<'_, SessionStore>,
    watchers: State<'_, ProjectWatchRegistry>,
) -> Result<WriteFieldOutcome, EditorError> {
    watchers.suppress_internal_write_events(session_id);
    let result = store.write_field(session_id, &coordinate, &field_path, &new_value);
    if result.is_err() {
        watchers.clear_internal_write_suppression(session_id);
    }
    result
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
fn edit_collection(
    session_id: u32,
    coordinate: RecordCoordinate,
    field_path: Vec<CfdPathSegment>,
    edit: CollectionEdit,
    store: State<'_, SessionStore>,
    watchers: State<'_, ProjectWatchRegistry>,
) -> Result<WriteFieldOutcome, EditorError> {
    watchers.suppress_internal_write_events(session_id);
    let result = store.edit_collection(session_id, &coordinate, &field_path, edit);
    if result.is_err() {
        watchers.clear_internal_write_suppression(session_id);
    }
    result
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
fn insert_record(
    session_id: u32,
    file_path: String,
    record_key: String,
    actual_type: String,
    fields: CfdValue,
    store: State<'_, SessionStore>,
    watchers: State<'_, ProjectWatchRegistry>,
) -> Result<InsertRecordOutcome, EditorError> {
    watchers.suppress_internal_write_events(session_id);
    let result = store.insert_record(session_id, &file_path, &record_key, &actual_type, fields);
    if result.is_err() {
        watchers.clear_internal_write_suppression(session_id);
    }
    result
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
fn rename_record_key(
    session_id: u32,
    coordinate: RecordCoordinate,
    new_key: String,
    store: State<'_, SessionStore>,
    watchers: State<'_, ProjectWatchRegistry>,
) -> Result<RenameRecordOutcome, EditorError> {
    watchers.suppress_internal_write_events(session_id);
    let result = store.rename_record_key(session_id, &coordinate, &new_key);
    if result.is_err() {
        watchers.clear_internal_write_suppression(session_id);
    }
    result
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
fn delete_record(
    session_id: u32,
    coordinate: RecordCoordinate,
    store: State<'_, SessionStore>,
    watchers: State<'_, ProjectWatchRegistry>,
) -> Result<DeleteRecordOutcome, EditorError> {
    watchers.suppress_internal_write_events(session_id);
    let result = store.delete_record(session_id, &coordinate);
    if result.is_err() {
        watchers.clear_internal_write_suppression(session_id);
    }
    result
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
/// Start the CFD editor Tauri application.
///
/// # Errors
/// Returns a Tauri error if application setup, context generation, or the
/// runtime event loop fails to start.
pub fn run() -> tauri::Result<()> {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let store = SessionStore::new().map_err(|err| err.to_string())?;
            app.manage(store);
            app.manage(ProjectWatchRegistry::default());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            load_project,
            init_project,
            close_session,
            get_file_records,
            get_graph,
            get_enum_variants,
            get_ref_targets,
            make_default_object,
            write_field,
            edit_collection,
            insert_record,
            rename_record_key,
            delete_record,
        ])
        .run(tauri::generate_context!())
}
