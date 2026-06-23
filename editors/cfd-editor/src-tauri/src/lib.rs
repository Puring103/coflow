use std::path::PathBuf;

mod editor;

use editor::{
    EditorError, FieldPathSegment, FieldValue, FileRecords, GraphData, ProjectSnapshot,
    SessionStore, WriteFieldOutcome,
};
use tauri::{Manager, State};

#[tauri::command]
fn load_project(
    yaml_path: String,
    store: State<'_, SessionStore>,
) -> Result<ProjectSnapshot, EditorError> {
    store.load_project(&PathBuf::from(yaml_path))
}

#[tauri::command]
fn init_project(
    dir: String,
    store: State<'_, SessionStore>,
) -> Result<ProjectSnapshot, EditorError> {
    store.init_project(&PathBuf::from(dir))
}

#[tauri::command]
fn close_session(session_id: u32, store: State<'_, SessionStore>) -> Result<(), EditorError> {
    store.close_session(session_id)
}

#[tauri::command]
fn get_file_records(
    session_id: u32,
    file_path: String,
    store: State<'_, SessionStore>,
) -> Result<FileRecords, EditorError> {
    store.get_file_records(session_id, &file_path)
}

#[tauri::command]
fn get_graph(
    session_id: u32,
    file_path: String,
    store: State<'_, SessionStore>,
) -> Result<GraphData, EditorError> {
    store.get_graph(session_id, &file_path)
}

#[tauri::command]
fn get_enum_variants(
    session_id: u32,
    enum_name: String,
    store: State<'_, SessionStore>,
) -> Result<Vec<String>, EditorError> {
    store.get_enum_variants(session_id, &enum_name)
}

#[tauri::command]
fn get_ref_targets(
    session_id: u32,
    target_type: String,
    store: State<'_, SessionStore>,
) -> Result<Vec<String>, EditorError> {
    store.get_ref_targets(session_id, &target_type)
}

#[tauri::command]
fn make_default_object(
    session_id: u32,
    type_name: String,
    store: State<'_, SessionStore>,
) -> Result<FieldValue, EditorError> {
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
) -> Result<WriteFieldOutcome, EditorError> {
    store.write_field(session_id, &file_path, &record_key, &field_path, &new_value)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let store = SessionStore::new().map_err(|err| err.to_string())?;
            app.manage(store);
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
