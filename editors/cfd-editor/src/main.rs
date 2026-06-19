#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use coflow_editor_core::commands::*;
use coflow_editor_core::types::*;
use std::sync::Mutex;

#[tauri::command]
fn load_project(
    state: tauri::State<'_, Mutex<SessionStore>>,
    yaml_path: String,
) -> Result<ProjectSnapshot, String> {
    load_project_inner(&state, &yaml_path)
}

#[tauri::command]
fn get_file_records(
    state: tauri::State<'_, Mutex<SessionStore>>,
    session_id: u32,
    file_path: String,
) -> Result<FileRecords, String> {
    get_file_records_inner(&state, session_id, &file_path)
}

#[tauri::command]
fn get_record(
    state: tauri::State<'_, Mutex<SessionStore>>,
    session_id: u32,
    file_path: String,
    record_key: String,
) -> Result<RecordRow, String> {
    get_record_inner(&state, session_id, &file_path, &record_key)
}

#[tauri::command]
fn get_graph(
    state: tauri::State<'_, Mutex<SessionStore>>,
    session_id: u32,
    file_path: String,
    expanded_keys: Option<Vec<String>>,
) -> Result<GraphData, String> {
    get_graph_inner(&state, session_id, &file_path, expanded_keys.as_deref().unwrap_or(&[]))
}

#[tauri::command]
fn rename_record(
    state: tauri::State<'_, Mutex<SessionStore>>,
    session_id: u32,
    file_path: String,
    old_key: String,
    new_key: String,
) -> Result<(), String> {
    rename_record_inner(&state, session_id, &file_path, &old_key, &new_key)
}

#[tauri::command]
fn get_diagnostics(
    state: tauri::State<'_, Mutex<SessionStore>>,
    session_id: u32,
) -> Result<Vec<DiagnosticItem>, String> {
    get_diagnostics_inner(&state, session_id)
}

#[tauri::command]
fn close_session(
    state: tauri::State<'_, Mutex<SessionStore>>,
    session_id: u32,
) -> Result<(), String> {
    close_session_inner(&state, session_id)
}

#[tauri::command]
fn write_field(
    state: tauri::State<'_, Mutex<SessionStore>>,
    session_id: u32,
    file_path: String,
    record_key: String,
    field_path: Vec<FieldPathSegment>,
    new_value: FieldValue,
) -> Result<(), String> {
    write_field_inner(
        &state,
        session_id,
        &file_path,
        &record_key,
        &field_path,
        &new_value,
    )
}

#[tauri::command]
fn create_record(
    state: tauri::State<'_, Mutex<SessionStore>>,
    session_id: u32,
    file_path: String,
    key: String,
    type_name: String,
) -> Result<RecordRow, String> {
    create_record_inner(&state, session_id, &file_path, &key, &type_name)
}

#[tauri::command]
fn delete_record(
    state: tauri::State<'_, Mutex<SessionStore>>,
    session_id: u32,
    file_path: String,
    record_key: String,
) -> Result<(), String> {
    delete_record_inner(&state, session_id, &file_path, &record_key)
}

#[tauri::command]
fn create_file(
    state: tauri::State<'_, Mutex<SessionStore>>,
    session_id: u32,
    rel_path: String,
) -> Result<FileTreeNode, String> {
    create_file_inner(&state, session_id, &rel_path)
}

#[tauri::command]
fn delete_file(
    state: tauri::State<'_, Mutex<SessionStore>>,
    session_id: u32,
    rel_path: String,
) -> Result<(), String> {
    delete_file_inner(&state, session_id, &rel_path)
}

#[tauri::command]
fn rename_file(
    state: tauri::State<'_, Mutex<SessionStore>>,
    session_id: u32,
    old_rel_path: String,
    new_rel_path: String,
) -> Result<(), String> {
    rename_file_inner(&state, session_id, &old_rel_path, &new_rel_path)
}

#[tauri::command]
fn get_all_type_names(
    state: tauri::State<'_, Mutex<SessionStore>>,
    session_id: u32,
) -> Result<Vec<String>, String> {
    get_all_type_names_inner(&state, session_id)
}

#[tauri::command]
fn get_ref_targets(
    state: tauri::State<'_, Mutex<SessionStore>>,
    session_id: u32,
    expected_type: String,
) -> Result<Vec<String>, String> {
    get_ref_targets_inner(&state, session_id, &expected_type)
}

#[tauri::command]
fn get_enum_variants(
    state: tauri::State<'_, Mutex<SessionStore>>,
    session_id: u32,
    enum_name: String,
) -> Result<Vec<String>, String> {
    get_enum_variants_inner(&state, session_id, &enum_name)
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .manage(Mutex::new(SessionStore::default()))
        .invoke_handler(tauri::generate_handler![
            load_project,
            get_file_records,
            get_record,
            get_graph,
            write_field,
            create_record,
            delete_record,
            create_file,
            delete_file,
            close_session,
            get_diagnostics,
            rename_record,
            rename_file,
            get_all_type_names,
            get_enum_variants,
            get_ref_targets,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
