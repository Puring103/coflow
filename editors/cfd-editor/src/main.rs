#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use coflow_editor_core::commands::*;
use coflow_editor_core::types::{
    DiagnosticItem, FieldPathSegment, FieldSchema, FieldValue, FileRecords, FileTreeNode,
    GraphData, ProjectSnapshot, RecordBrief, RecordRow, SearchHit,
};
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

#[tauri::command]
fn duplicate_record(
    state: tauri::State<'_, Mutex<SessionStore>>,
    session_id: u32,
    file_path: String,
    src_key: String,
    new_key: String,
) -> Result<RecordRow, String> {
    duplicate_record_inner(&state, session_id, &file_path, &src_key, &new_key)
}

#[tauri::command]
fn get_all_records_brief(
    state: tauri::State<'_, Mutex<SessionStore>>,
    session_id: u32,
) -> Result<Vec<RecordBrief>, String> {
    get_all_records_brief_inner(&state, session_id)
}

#[tauri::command]
fn get_field_schemas(
    state: tauri::State<'_, Mutex<SessionStore>>,
    session_id: u32,
    type_name: String,
) -> Result<Vec<FieldSchema>, String> {
    get_field_schemas_inner(&state, session_id, &type_name)
}

#[tauri::command]
fn get_record_source(
    state: tauri::State<'_, Mutex<SessionStore>>,
    session_id: u32,
    file_path: String,
    record_key: String,
) -> Result<String, String> {
    get_record_source_inner(&state, session_id, &file_path, &record_key)
}

#[tauri::command]
fn move_record(
    state: tauri::State<'_, Mutex<SessionStore>>,
    session_id: u32,
    src_file: String,
    dst_file: String,
    record_key: String,
) -> Result<RecordRow, String> {
    move_record_inner(&state, session_id, &src_file, &dst_file, &record_key)
}

#[tauri::command]
fn search_records(
    state: tauri::State<'_, Mutex<SessionStore>>,
    session_id: u32,
    query: String,
    limit: Option<usize>,
) -> Result<Vec<SearchHit>, String> {
    search_records_inner(&state, session_id, &query, limit.unwrap_or(100))
}

#[tauri::command]
fn import_record_source(
    state: tauri::State<'_, Mutex<SessionStore>>,
    session_id: u32,
    file_path: String,
    source: String,
) -> Result<Vec<String>, String> {
    import_record_source_inner(&state, session_id, &file_path, &source)
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
            duplicate_record,
            get_all_records_brief,
            get_field_schemas,
            get_record_source,
            move_record,
            search_records,
            import_record_source,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
