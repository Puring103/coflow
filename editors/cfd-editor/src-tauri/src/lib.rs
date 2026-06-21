use std::path::PathBuf;

use coflow_editor_core::{FileRecords, GraphData, ProjectSnapshot, RecordRow, SessionStore};
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
