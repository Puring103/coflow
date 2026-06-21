use std::path::PathBuf;

use coflow_editor_core::{
    FieldPathSegment, FieldValue, FileRecords, GraphData, ProjectSnapshot, RecordRow, SessionStore,
};
use tauri::{Manager, State};

#[tauri::command]
fn load_project(yaml_path: String, store: State<'_, SessionStore>) -> Result<ProjectSnapshot, String> {
    store.load_project(&PathBuf::from(yaml_path))
}

#[tauri::command]
fn close_session(session_id: u32, store: State<'_, SessionStore>) -> Result<(), String> {
    store.close_session(session_id)
}

#[tauri::command]
fn get_file_records(
    session_id: u32,
    file_path: String,
    store: State<'_, SessionStore>,
) -> Result<FileRecords, String> {
    store.get_file_records(session_id, &file_path)
}

#[tauri::command]
fn get_record(
    session_id: u32,
    file_path: String,
    record_key: String,
    store: State<'_, SessionStore>,
) -> Result<RecordRow, String> {
    store.get_record(session_id, &file_path, &record_key)
}

#[tauri::command]
fn get_graph(
    session_id: u32,
    file_path: String,
    store: State<'_, SessionStore>,
) -> Result<GraphData, String> {
    store.get_graph(session_id, &file_path)
}

#[tauri::command]
fn get_enum_variants(
    session_id: u32,
    enum_name: String,
    store: State<'_, SessionStore>,
) -> Result<Vec<String>, String> {
    store.get_enum_variants(session_id, &enum_name)
}

#[tauri::command]
fn get_ref_targets(
    session_id: u32,
    target_type: String,
    store: State<'_, SessionStore>,
) -> Result<Vec<String>, String> {
    store.get_ref_targets(session_id, &target_type)
}

#[tauri::command]
fn make_default_object(
    session_id: u32,
    type_name: String,
    store: State<'_, SessionStore>,
) -> Result<FieldValue, String> {
    store.make_default_object(session_id, &type_name)
}

#[tauri::command]
fn write_field(
    session_id: u32,
    file_path: String,
    record_key: String,
    field_path: Vec<FieldPathSegment>,
    new_value: FieldValue,
    store: State<'_, SessionStore>,
) -> Result<RecordRow, String> {
    store.write_field(session_id, &file_path, &record_key, &field_path, &new_value)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            app.manage(SessionStore::new());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            load_project,
            close_session,
            get_file_records,
            get_record,
            get_graph,
            get_enum_variants,
            get_ref_targets,
            make_default_object,
            write_field,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
