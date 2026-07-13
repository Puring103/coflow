#![allow(clippy::multiple_crate_versions)]

use std::path::PathBuf;

pub mod editor;
mod host;
mod watcher;

use coflow_data_model::{CfdPathSegment, CfdValue};
use coflow_runtime::RecordCoordinate;
use editor::{
    CollectionEdit, CreateRecordDraft, DeleteRecordOutcome, EditorError, FileRecords, GraphData,
    GraphQuery, InsertRecordOutcome, ProjectSnapshot, RefTarget, RenameRecordOutcome,
    WriteFieldOutcome,
};
use host::EditorHost;
use tauri::{AppHandle, Manager, State};

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
fn load_project(
    yaml_path: String,
    host: State<'_, EditorHost>,
    app: AppHandle,
) -> Result<ProjectSnapshot, EditorError> {
    host.load_project(app, &PathBuf::from(yaml_path))
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
fn init_project(
    dir: String,
    host: State<'_, EditorHost>,
    app: AppHandle,
) -> Result<ProjectSnapshot, EditorError> {
    host.init_project(app, &PathBuf::from(dir))
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
fn close_session(
    session_id: u32,
    host: State<'_, EditorHost>,
) -> Result<(), EditorError> {
    host.close_session(session_id)
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
fn get_file_records(
    session_id: u32,
    file_path: String,
    host: State<'_, EditorHost>,
) -> Result<FileRecords, EditorError> {
    host.sessions().get_file_records(session_id, &file_path)
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
fn get_graph(
    session_id: u32,
    file_path: String,
    depth: Option<usize>,
    limit: Option<usize>,
    host: State<'_, EditorHost>,
) -> Result<GraphData, EditorError> {
    host.sessions().get_graph(
        session_id,
        &GraphQuery {
            file_path,
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
    host: State<'_, EditorHost>,
) -> Result<Vec<String>, EditorError> {
    host.sessions().get_enum_variants(session_id, &enum_name)
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
fn get_ref_targets(
    session_id: u32,
    target_type: String,
    host: State<'_, EditorHost>,
) -> Result<Vec<RefTarget>, EditorError> {
    host.sessions().get_ref_targets(session_id, &target_type)
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
fn make_default_object(
    session_id: u32,
    type_name: String,
    host: State<'_, EditorHost>,
) -> Result<CfdValue, EditorError> {
    host.sessions().make_default_object(session_id, &type_name)
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
fn create_record_draft(
    session_id: u32,
    actual_type: String,
    host: State<'_, EditorHost>,
) -> Result<CreateRecordDraft, EditorError> {
    host.sessions().create_record_draft(session_id, &actual_type)
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
fn write_field(
    session_id: u32,
    coordinate: RecordCoordinate,
    field_path: Vec<CfdPathSegment>,
    new_value: CfdValue,
    host: State<'_, EditorHost>,
) -> Result<WriteFieldOutcome, EditorError> {
    host.sessions()
        .write_field(session_id, &coordinate, &field_path, &new_value)
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
fn edit_collection(
    session_id: u32,
    coordinate: RecordCoordinate,
    field_path: Vec<CfdPathSegment>,
    edit: CollectionEdit,
    host: State<'_, EditorHost>,
) -> Result<WriteFieldOutcome, EditorError> {
    host.sessions()
        .edit_collection(session_id, &coordinate, &field_path, edit)
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
fn insert_record(
    session_id: u32,
    file_path: String,
    record_key: String,
    actual_type: String,
    fields: CfdValue,
    host: State<'_, EditorHost>,
) -> Result<InsertRecordOutcome, EditorError> {
    host.sessions()
        .insert_record(session_id, &file_path, &record_key, &actual_type, fields)
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
fn rename_record_key(
    session_id: u32,
    coordinate: RecordCoordinate,
    new_key: String,
    host: State<'_, EditorHost>,
) -> Result<RenameRecordOutcome, EditorError> {
    host.sessions()
        .rename_record_key(session_id, &coordinate, &new_key)
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
fn delete_record(
    session_id: u32,
    coordinate: RecordCoordinate,
    host: State<'_, EditorHost>,
) -> Result<DeleteRecordOutcome, EditorError> {
    host.sessions().delete_record(session_id, &coordinate)
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
            let host = EditorHost::new().map_err(|err| err.to_string())?;
            app.manage(host);
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
            create_record_draft,
            write_field,
            edit_collection,
            insert_record,
            rename_record_key,
            delete_record,
        ])
        .run(tauri::generate_context!())
}
