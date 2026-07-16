#![allow(clippy::multiple_crate_versions, clippy::unreachable)]

use std::collections::BTreeMap;
use std::path::PathBuf;

pub mod editor;
mod host;
mod watcher;

use coflow_data_model::{CfdPathSegment, CfdValue};
use coflow_runtime::{DimensionValueCoordinate, DimensionValueView, RecordCoordinate};
use editor::{
    CollectionEdit, CreateRecordDraft, DeleteRecordOutcome, EditorError, FileRecords, GraphData,
    EditorProjectSettings, GraphQuery, InsertRecordOutcome, ProjectSnapshot, RefTarget,
    RenameRecordOutcome, WriteDimensionValueOutcome, WriteFieldOutcome,
};
use host::EditorHost;
use tauri::{AppHandle, Manager, State};

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
async fn load_project(
    yaml_path: String,
    host: State<'_, EditorHost>,
    app: AppHandle,
) -> Result<ProjectSnapshot, EditorError> {
    let host = host.inner().clone();
    run_blocking(move || host.load_project(app, &PathBuf::from(yaml_path))).await
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
async fn init_project(
    dir: String,
    host: State<'_, EditorHost>,
    app: AppHandle,
) -> Result<ProjectSnapshot, EditorError> {
    let host = host.inner().clone();
    run_blocking(move || host.init_project(app, &PathBuf::from(dir))).await
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
async fn close_session(session_id: u32, host: State<'_, EditorHost>) -> Result<(), EditorError> {
    let host = host.inner().clone();
    run_blocking(move || host.close_session(session_id)).await
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
async fn get_project_settings(
    session_id: u32,
    host: State<'_, EditorHost>,
) -> Result<EditorProjectSettings, EditorError> {
    let host = host.inner().clone();
    run_blocking(move || host.sessions().get_project_settings(session_id)).await
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
async fn set_table_column_widths(
    session_id: u32,
    file_path: String,
    actual_type: String,
    widths: BTreeMap<String, f64>,
    host: State<'_, EditorHost>,
) -> Result<EditorProjectSettings, EditorError> {
    let host = host.inner().clone();
    run_blocking(move || {
        host.sessions()
            .set_table_column_widths(session_id, file_path, actual_type, widths)
    })
    .await
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
async fn get_file_records(
    session_id: u32,
    file_path: String,
    host: State<'_, EditorHost>,
) -> Result<FileRecords, EditorError> {
    let host = host.inner().clone();
    run_blocking(move || host.sessions().get_file_records(session_id, &file_path)).await
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
async fn get_graph(
    session_id: u32,
    file_path: String,
    depth: Option<usize>,
    limit: Option<usize>,
    host: State<'_, EditorHost>,
) -> Result<GraphData, EditorError> {
    let host = host.inner().clone();
    run_blocking(move || {
        host.sessions().get_graph(
            session_id,
            &GraphQuery {
                file_path,
                depth,
                limit,
            },
        )
    })
    .await
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
async fn get_enum_variants(
    session_id: u32,
    enum_name: String,
    host: State<'_, EditorHost>,
) -> Result<Vec<String>, EditorError> {
    let host = host.inner().clone();
    run_blocking(move || host.sessions().get_enum_variants(session_id, &enum_name)).await
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
async fn get_ref_targets(
    session_id: u32,
    target_type: String,
    host: State<'_, EditorHost>,
) -> Result<Vec<RefTarget>, EditorError> {
    let host = host.inner().clone();
    run_blocking(move || host.sessions().get_ref_targets(session_id, &target_type)).await
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
async fn make_default_object(
    session_id: u32,
    type_name: String,
    host: State<'_, EditorHost>,
) -> Result<CfdValue, EditorError> {
    let host = host.inner().clone();
    run_blocking(move || host.sessions().make_default_object(session_id, &type_name)).await
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
async fn create_record_draft(
    session_id: u32,
    actual_type: String,
    host: State<'_, EditorHost>,
) -> Result<CreateRecordDraft, EditorError> {
    let host = host.inner().clone();
    run_blocking(move || {
        host.sessions()
            .create_record_draft(session_id, &actual_type)
    })
    .await
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
async fn render_cell_text(
    session_id: u32,
    coordinate: RecordCoordinate,
    field_path: Vec<CfdPathSegment>,
    host: State<'_, EditorHost>,
) -> Result<String, EditorError> {
    let host = host.inner().clone();
    run_blocking(move || {
        host.sessions()
            .render_cell_text(session_id, &coordinate, &field_path)
    })
    .await
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
async fn parse_cell_text(
    session_id: u32,
    coordinate: RecordCoordinate,
    field_path: Vec<CfdPathSegment>,
    text: String,
    host: State<'_, EditorHost>,
) -> Result<CfdValue, EditorError> {
    let host = host.inner().clone();
    run_blocking(move || {
        host.sessions()
            .parse_cell_text(session_id, &coordinate, &field_path, &text)
    })
    .await
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
async fn write_field(
    session_id: u32,
    coordinate: RecordCoordinate,
    field_path: Vec<CfdPathSegment>,
    new_value: CfdValue,
    host: State<'_, EditorHost>,
) -> Result<WriteFieldOutcome, EditorError> {
    let host = host.inner().clone();
    run_blocking(move || {
        host.sessions()
            .write_field(session_id, &coordinate, &field_path, &new_value)
    })
    .await
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
async fn get_dimension_value(
    session_id: u32,
    coordinate: DimensionValueCoordinate,
    host: State<'_, EditorHost>,
) -> Result<DimensionValueView, EditorError> {
    let host = host.inner().clone();
    run_blocking(move || host.sessions().get_dimension_value(session_id, &coordinate)).await
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
async fn write_dimension_value(
    session_id: u32,
    coordinate: DimensionValueCoordinate,
    expected_value: coflow_runtime::DimensionValueState,
    new_value: coflow_runtime::DimensionValueState,
    host: State<'_, EditorHost>,
) -> Result<WriteDimensionValueOutcome, EditorError> {
    let host = host.inner().clone();
    run_blocking(move || {
        host.sessions()
            .write_dimension_value(session_id, &coordinate, &expected_value, &new_value)
    })
    .await
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
async fn edit_collection(
    session_id: u32,
    coordinate: RecordCoordinate,
    field_path: Vec<CfdPathSegment>,
    edit: CollectionEdit,
    host: State<'_, EditorHost>,
) -> Result<WriteFieldOutcome, EditorError> {
    let host = host.inner().clone();
    run_blocking(move || {
        host.sessions()
            .edit_collection(session_id, &coordinate, &field_path, edit)
    })
    .await
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
async fn insert_record(
    session_id: u32,
    file_path: String,
    record_key: String,
    actual_type: String,
    fields: CfdValue,
    host: State<'_, EditorHost>,
) -> Result<InsertRecordOutcome, EditorError> {
    let host = host.inner().clone();
    run_blocking(move || {
        host.sessions()
            .insert_record(session_id, &file_path, &record_key, &actual_type, fields)
    })
    .await
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
async fn rename_record_key(
    session_id: u32,
    coordinate: RecordCoordinate,
    new_key: String,
    host: State<'_, EditorHost>,
) -> Result<RenameRecordOutcome, EditorError> {
    let host = host.inner().clone();
    run_blocking(move || {
        host.sessions()
            .rename_record_key(session_id, &coordinate, &new_key)
    })
    .await
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
async fn delete_record(
    session_id: u32,
    coordinate: RecordCoordinate,
    host: State<'_, EditorHost>,
) -> Result<DeleteRecordOutcome, EditorError> {
    let host = host.inner().clone();
    run_blocking(move || host.sessions().delete_record(session_id, &coordinate)).await
}

async fn run_blocking<T>(
    work: impl FnOnce() -> Result<T, EditorError> + Send + 'static,
) -> Result<T, EditorError>
where
    T: Send + 'static,
{
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| EditorError::other(format!("background command failed: {error}")))?
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
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let host = EditorHost::new().map_err(|err| err.to_string())?;
            app.manage(host);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            load_project,
            init_project,
            close_session,
            get_project_settings,
            set_table_column_widths,
            get_file_records,
            get_graph,
            get_enum_variants,
            get_ref_targets,
            make_default_object,
            create_record_draft,
            render_cell_text,
            parse_cell_text,
            write_field,
            get_dimension_value,
            write_dimension_value,
            edit_collection,
            insert_record,
            rename_record_key,
            delete_record,
        ])
        .run(tauri::generate_context!())
}
