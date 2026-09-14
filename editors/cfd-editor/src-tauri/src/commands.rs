//! Tauri 命令适配层：只负责参数接收、阻塞任务调度与 host 调用。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use cfd_editor_core::EditorHost;
use coflow_runtime::{CfdPathSegment, CfdValue, FlatDiagnostic};
use coflow_runtime::{DimensionValueCoordinate, DimensionValueView, ProjectDiff, RecordCoordinate};
use tauri::State;

use crate::editor::{
    BatchWriteFieldInput, BatchWriteFieldOutcome, CollectionEdit, CreateRecordDraft,
    DeleteRecordOutcome, DimensionFileRecords, EditorError, EditorProjectSettings,
    EditorRecordGroup, EditorWorkspaceState, EnumVariantOption, FileRecords, FunctionDocumentState,
    GraphData, GraphQuery, InsertRecordOutcome, LanguageCompletion, LanguageDocumentState,
    LanguageFormattingResult, LanguagePosition, PluginSchemaType, ProjectBootstrap,
    ProjectSearchMode, ProjectSearchResults, RecordRow, RefTarget, RenameRecordOutcome,
    ReorderRecordsOutcome, ViewConfig, WriteDimensionValueOutcome, WriteFieldOutcome,
};
#[tauri::command]
pub(crate) async fn load_project(
    yaml_path: String,
    host: State<'_, EditorHost>,
) -> Result<ProjectBootstrap, EditorError> {
    run_host_command(host, move |host| {
        host.load_project(&PathBuf::from(yaml_path))
    })
    .await
}

#[tauri::command]
pub(crate) async fn init_project(
    dir: String,
    host: State<'_, EditorHost>,
) -> Result<ProjectBootstrap, EditorError> {
    run_host_command(host, move |host| host.init_project(&PathBuf::from(dir))).await
}

#[tauri::command]
pub(crate) async fn close_session(
    session_id: u32,
    host: State<'_, EditorHost>,
) -> Result<(), EditorError> {
    run_host_command(host, move |host| host.close_session(session_id)).await
}

#[tauri::command]
pub(crate) async fn reload_session(
    session_id: u32,
    host: State<'_, EditorHost>,
) -> Result<ProjectBootstrap, EditorError> {
    run_host_command(host, move |host| host.reload_session(session_id)).await
}

#[tauri::command]
pub(crate) async fn add_project_input(
    session_id: u32,
    kind: String,
    path: String,
    host: State<'_, EditorHost>,
) -> Result<ProjectBootstrap, EditorError> {
    let kind = project_input_kind(&kind)?;
    run_host_command(host, move |host| {
        host.sessions()
            .add_project_input(session_id, kind, &PathBuf::from(path))
    })
    .await
}

fn project_input_kind(kind: &str) -> Result<coflow_runtime::ProjectInputKind, EditorError> {
    match kind {
        "schema" => Ok(coflow_runtime::ProjectInputKind::Schema),
        "data" => Ok(coflow_runtime::ProjectInputKind::Data),
        _ => Err(EditorError::other(
            "project input kind must be schema or data",
        )),
    }
}

#[tauri::command]
pub(crate) async fn create_project_file(
    session_id: u32,
    kind: String,
    parent_path: String,
    file_name: String,
    host: State<'_, EditorHost>,
) -> Result<ProjectBootstrap, EditorError> {
    let kind = project_input_kind(&kind)?;
    run_host_command(host, move |host| {
        host.sessions()
            .create_project_file(session_id, kind, Path::new(&parent_path), &file_name)
    })
    .await
}

#[tauri::command]
pub(crate) async fn delete_project_entry(
    session_id: u32,
    path: String,
    host: State<'_, EditorHost>,
) -> Result<ProjectBootstrap, EditorError> {
    run_host_command(host, move |host| {
        host.sessions()
            .delete_project_entry(session_id, Path::new(&path))
    })
    .await
}

#[tauri::command]
pub(crate) async fn get_project_settings(
    session_id: u32,
    host: State<'_, EditorHost>,
) -> Result<EditorProjectSettings, EditorError> {
    run_host_command(host, move |host| {
        host.sessions().get_project_settings(session_id)
    })
    .await
}

#[tauri::command]
pub(crate) async fn get_dimension_file_records(
    session_id: u32,
    file_path: String,
    host: State<'_, EditorHost>,
) -> Result<DimensionFileRecords, EditorError> {
    run_host_command(host, move |host| {
        host.sessions()
            .get_dimension_file_records(session_id, &file_path)
    })
    .await
}

#[tauri::command]
pub(crate) async fn set_graph_positions(
    session_id: u32,
    view_key: String,
    positions: BTreeMap<String, [f64; 2]>,
    host: State<'_, EditorHost>,
) -> Result<(), EditorError> {
    run_host_command(host, move |host| {
        host.sessions()
            .set_graph_positions(session_id, view_key, positions)
    })
    .await
}

#[tauri::command]
pub(crate) async fn set_default_table_column_widths(
    session_id: u32,
    file_path: String,
    actual_type: String,
    widths: BTreeMap<String, f64>,
    host: State<'_, EditorHost>,
) -> Result<EditorProjectSettings, EditorError> {
    run_host_command(host, move |host| {
        host.sessions()
            .set_default_table_column_widths(session_id, file_path, actual_type, widths)
    })
    .await
}

#[tauri::command]
pub(crate) async fn set_short_name_field(
    session_id: u32,
    actual_type: String,
    field: Option<String>,
    host: State<'_, EditorHost>,
) -> Result<EditorProjectSettings, EditorError> {
    run_host_command(host, move |host| {
        host.sessions()
            .set_short_name_field(session_id, actual_type, field)
    })
    .await
}

#[tauri::command]
pub(crate) async fn set_view_order(
    session_id: u32,
    file_path: String,
    actual_type: String,
    order: Vec<String>,
    host: State<'_, EditorHost>,
) -> Result<EditorProjectSettings, EditorError> {
    run_host_command(host, move |host| {
        host.sessions()
            .set_view_order(session_id, file_path, actual_type, order)
    })
    .await
}

#[tauri::command]
pub(crate) async fn set_views(
    session_id: u32,
    file_path: String,
    actual_type: String,
    views: Vec<ViewConfig>,
    host: State<'_, EditorHost>,
) -> Result<EditorProjectSettings, EditorError> {
    run_host_command(host, move |host| {
        host.sessions()
            .set_views(session_id, file_path, actual_type, views)
    })
    .await
}

#[tauri::command]
pub(crate) async fn set_view_column_widths(
    session_id: u32,
    file_path: String,
    actual_type: String,
    view_id: String,
    widths: BTreeMap<String, f64>,
    host: State<'_, EditorHost>,
) -> Result<EditorProjectSettings, EditorError> {
    run_host_command(host, move |host| {
        host.sessions()
            .set_view_column_widths(session_id, file_path, actual_type, view_id, widths)
    })
    .await
}

#[tauri::command]
pub(crate) async fn set_record_groups(
    session_id: u32,
    file_path: String,
    actual_type: String,
    groups: Vec<EditorRecordGroup>,
    host: State<'_, EditorHost>,
) -> Result<EditorProjectSettings, EditorError> {
    run_host_command(host, move |host| {
        host.sessions()
            .set_record_groups(session_id, file_path, actual_type, groups)
    })
    .await
}

#[tauri::command]
pub(crate) async fn set_workspace(
    session_id: u32,
    workspace: EditorWorkspaceState,
    host: State<'_, EditorHost>,
) -> Result<EditorProjectSettings, EditorError> {
    run_host_command(host, move |host| {
        host.sessions().set_workspace(session_id, workspace)
    })
    .await
}

#[tauri::command]
pub(crate) async fn check_project(
    session_id: u32,
    host: State<'_, EditorHost>,
) -> Result<String, EditorError> {
    run_host_command(host, move |host| host.sessions().check_project(session_id)).await
}

#[tauri::command]
pub(crate) async fn build_project(
    session_id: u32,
    host: State<'_, EditorHost>,
) -> Result<String, EditorError> {
    run_host_command(host, move |host| host.sessions().build_project(session_id)).await
}

#[tauri::command]
pub(crate) async fn build_project_status(
    session_id: u32,
    host: State<'_, EditorHost>,
) -> Result<bool, EditorError> {
    run_host_command(host, move |host| {
        host.sessions().build_project_status(session_id)
    })
    .await
}

#[tauri::command]
pub(crate) async fn get_project_diff(
    session_id: u32,
    host: State<'_, EditorHost>,
) -> Result<ProjectDiff, EditorError> {
    run_host_command(host, move |host| host.sessions().project_diff(session_id)).await
}

#[tauri::command]
pub(crate) async fn open_source_file(
    session_id: u32,
    file_path: String,
    host: State<'_, EditorHost>,
) -> Result<(), EditorError> {
    run_host_command(host, move |host| {
        let path = host.sessions().source_file_path(session_id, &file_path)?;
        open_with_default_application(&path)
    })
    .await
}

#[tauri::command]
pub(crate) async fn read_source_text(
    session_id: u32,
    file_path: String,
    host: State<'_, EditorHost>,
) -> Result<String, EditorError> {
    run_host_command(host, move |host| {
        host.sessions().read_source_text(session_id, &file_path)
    })
    .await
}

#[tauri::command]
pub(crate) async fn highlight_source_snapshot(
    session_id: u32,
    file_path: String,
    source: String,
    host: State<'_, EditorHost>,
) -> Result<LanguageDocumentState, EditorError> {
    run_host_command(host, move |host| {
        host.sessions()
            .highlight_source_snapshot(session_id, &file_path, &source)
    })
    .await
}

#[tauri::command]
pub(crate) async fn sync_language_document(
    session_id: u32,
    file_path: String,
    source: String,
    version: i64,
    host: State<'_, EditorHost>,
) -> Result<LanguageDocumentState, EditorError> {
    run_host_command(host, move |host| {
        host.sessions()
            .sync_language_document(session_id, &file_path, &source, version)
    })
    .await
}

#[tauri::command]
pub(crate) async fn validate_source_text(
    session_id: u32,
    file_path: String,
    source: String,
    host: State<'_, EditorHost>,
) -> Result<Vec<FlatDiagnostic>, EditorError> {
    run_host_command(host, move |host| {
        host.sessions()
            .validate_source_text(session_id, &file_path, &source)
    })
    .await
}

#[tauri::command]
pub(crate) async fn complete_language_document(
    session_id: u32,
    file_path: String,
    source: String,
    version: i64,
    position: LanguagePosition,
    host: State<'_, EditorHost>,
) -> Result<Vec<LanguageCompletion>, EditorError> {
    run_host_command(host, move |host| {
        host.sessions()
            .complete_language_document(session_id, &file_path, &source, version, &position)
    })
    .await
}

#[tauri::command]
pub(crate) async fn format_language_document(
    session_id: u32,
    file_path: String,
    source: String,
    version: i64,
    host: State<'_, EditorHost>,
) -> Result<LanguageFormattingResult, EditorError> {
    run_host_command(host, move |host| {
        host.sessions()
            .format_language_document(session_id, &file_path, &source, version)
    })
    .await
}

#[tauri::command]
pub(crate) async fn close_language_document(
    session_id: u32,
    file_path: String,
    host: State<'_, EditorHost>,
) -> Result<(), EditorError> {
    run_host_command(host, move |host| {
        host.sessions()
            .close_language_document(session_id, &file_path)
    })
    .await
}

#[tauri::command]
pub(crate) async fn function_document(
    session_id: u32,
    source: String,
    body: Option<String>,
    host: State<'_, EditorHost>,
) -> Result<FunctionDocumentState, EditorError> {
    run_host_command(host, move |host| {
        host.sessions()
            .function_document(session_id, &source, body.as_deref())
    })
    .await
}

#[tauri::command]
pub(crate) async fn write_source_text(
    session_id: u32,
    file_path: String,
    source: String,
    host: State<'_, EditorHost>,
) -> Result<ProjectBootstrap, EditorError> {
    run_host_command(host, move |host| {
        host.sessions()
            .write_source_text(session_id, &file_path, &source)
    })
    .await
}

#[tauri::command]
pub(crate) async fn get_file_records(
    session_id: u32,
    file_path: String,
    host: State<'_, EditorHost>,
) -> Result<FileRecords, EditorError> {
    run_host_command(host, move |host| {
        host.sessions().get_file_records(session_id, &file_path)
    })
    .await
}

#[tauri::command]
pub(crate) async fn search_records(
    session_id: u32,
    query: String,
    mode: ProjectSearchMode,
    limit: usize,
    host: State<'_, EditorHost>,
) -> Result<ProjectSearchResults, EditorError> {
    run_host_command(host, move |host| {
        host.sessions()
            .search_records(session_id, &query, mode, limit)
    })
    .await
}

#[tauri::command]
pub(crate) async fn get_plugin_schema(
    session_id: u32,
    host: State<'_, EditorHost>,
) -> Result<Vec<PluginSchemaType>, EditorError> {
    run_host_command(host, move |host| {
        host.sessions().get_plugin_schema(session_id)
    })
    .await
}

#[tauri::command]
pub(crate) async fn get_plugin_records_by_type(
    session_id: u32,
    type_name: String,
    host: State<'_, EditorHost>,
) -> Result<Vec<RecordRow>, EditorError> {
    run_host_command(host, move |host| {
        host.sessions()
            .get_plugin_records_by_type(session_id, &type_name)
    })
    .await
}

#[tauri::command]
pub(crate) async fn get_graph(
    session_id: u32,
    file_path: String,
    depth: Option<usize>,
    limit: Option<usize>,
    host: State<'_, EditorHost>,
) -> Result<GraphData, EditorError> {
    run_host_command(host, move |host| {
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

#[tauri::command]
pub(crate) async fn get_enum_variants(
    session_id: u32,
    enum_name: String,
    host: State<'_, EditorHost>,
) -> Result<Vec<EnumVariantOption>, EditorError> {
    run_host_command(host, move |host| {
        host.sessions().get_enum_variants(session_id, &enum_name)
    })
    .await
}

#[tauri::command]
pub(crate) async fn get_ref_targets(
    session_id: u32,
    target_type: String,
    host: State<'_, EditorHost>,
) -> Result<Vec<RefTarget>, EditorError> {
    run_host_command(host, move |host| {
        host.sessions().get_ref_targets(session_id, &target_type)
    })
    .await
}

#[tauri::command]
pub(crate) async fn make_default_object(
    session_id: u32,
    type_name: String,
    host: State<'_, EditorHost>,
) -> Result<CfdValue, EditorError> {
    run_host_command(host, move |host| {
        host.sessions().make_default_object(session_id, &type_name)
    })
    .await
}

#[tauri::command]
pub(crate) async fn create_record_draft(
    session_id: u32,
    actual_type: String,
    host: State<'_, EditorHost>,
) -> Result<CreateRecordDraft, EditorError> {
    run_host_command(host, move |host| {
        host.sessions()
            .create_record_draft(session_id, &actual_type)
    })
    .await
}

#[tauri::command]
pub(crate) async fn render_cell_text(
    session_id: u32,
    coordinate: RecordCoordinate,
    field_path: Vec<CfdPathSegment>,
    host: State<'_, EditorHost>,
) -> Result<String, EditorError> {
    run_host_command(host, move |host| {
        host.sessions()
            .render_cell_text(session_id, &coordinate, &field_path)
    })
    .await
}

#[tauri::command]
pub(crate) async fn parse_cell_text(
    session_id: u32,
    coordinate: RecordCoordinate,
    field_path: Vec<CfdPathSegment>,
    text: String,
    host: State<'_, EditorHost>,
) -> Result<CfdValue, EditorError> {
    run_host_command(host, move |host| {
        host.sessions()
            .parse_cell_text(session_id, &coordinate, &field_path, &text)
    })
    .await
}

#[tauri::command]
pub(crate) async fn write_field(
    session_id: u32,
    coordinate: RecordCoordinate,
    field_path: Vec<CfdPathSegment>,
    new_value: CfdValue,
    host: State<'_, EditorHost>,
) -> Result<WriteFieldOutcome, EditorError> {
    run_host_command(host, move |host| {
        host.sessions()
            .write_field(session_id, &coordinate, &field_path, &new_value)
    })
    .await
}

#[tauri::command]
pub(crate) async fn write_fields(
    session_id: u32,
    writes: Vec<BatchWriteFieldInput>,
    host: State<'_, EditorHost>,
) -> Result<BatchWriteFieldOutcome, EditorError> {
    run_host_command(host, move |host| {
        host.sessions().write_fields(session_id, &writes)
    })
    .await
}

#[tauri::command]
pub(crate) async fn get_dimension_value(
    session_id: u32,
    coordinate: DimensionValueCoordinate,
    host: State<'_, EditorHost>,
) -> Result<DimensionValueView, EditorError> {
    run_host_command(host, move |host| {
        host.sessions().get_dimension_value(session_id, &coordinate)
    })
    .await
}

#[tauri::command]
pub(crate) async fn write_dimension_value(
    session_id: u32,
    coordinate: DimensionValueCoordinate,
    expected_value: coflow_runtime::DimensionValueState,
    new_value: coflow_runtime::DimensionValueState,
    host: State<'_, EditorHost>,
) -> Result<WriteDimensionValueOutcome, EditorError> {
    run_host_command(host, move |host| {
        host.sessions()
            .write_dimension_value(session_id, &coordinate, &expected_value, &new_value)
    })
    .await
}

#[tauri::command]
pub(crate) async fn edit_collection(
    session_id: u32,
    coordinate: RecordCoordinate,
    field_path: Vec<CfdPathSegment>,
    edit: CollectionEdit,
    host: State<'_, EditorHost>,
) -> Result<WriteFieldOutcome, EditorError> {
    run_host_command(host, move |host| {
        host.sessions()
            .edit_collection(session_id, &coordinate, &field_path, edit)
    })
    .await
}

#[tauri::command]
pub(crate) async fn insert_record(
    session_id: u32,
    file_path: String,
    record_key: String,
    actual_type: String,
    fields: CfdValue,
    host: State<'_, EditorHost>,
) -> Result<InsertRecordOutcome, EditorError> {
    run_host_command(host, move |host| {
        host.sessions()
            .insert_record(session_id, &file_path, &record_key, &actual_type, fields)
    })
    .await
}

#[tauri::command]
pub(crate) async fn rename_record_key(
    session_id: u32,
    coordinate: RecordCoordinate,
    new_key: String,
    host: State<'_, EditorHost>,
) -> Result<RenameRecordOutcome, EditorError> {
    run_host_command(host, move |host| {
        host.sessions()
            .rename_record_key(session_id, &coordinate, &new_key)
    })
    .await
}

#[tauri::command]
pub(crate) async fn delete_record(
    session_id: u32,
    coordinate: RecordCoordinate,
    host: State<'_, EditorHost>,
) -> Result<DeleteRecordOutcome, EditorError> {
    run_host_command(host, move |host| {
        host.sessions().delete_record(session_id, &coordinate)
    })
    .await
}

#[tauri::command]
pub(crate) async fn swap_records(
    session_id: u32,
    first: RecordCoordinate,
    second: RecordCoordinate,
    host: State<'_, EditorHost>,
) -> Result<ReorderRecordsOutcome, EditorError> {
    run_host_command(host, move |host| {
        host.sessions().swap_records(session_id, &first, &second)
    })
    .await
}

#[tauri::command]
pub(crate) async fn move_record(
    session_id: u32,
    coordinate: RecordCoordinate,
    target_index: usize,
    host: State<'_, EditorHost>,
) -> Result<ReorderRecordsOutcome, EditorError> {
    run_host_command(host, move |host| {
        host.sessions()
            .move_record(session_id, &coordinate, target_index)
    })
    .await
}

#[tauri::command]
pub(crate) async fn transfer_record(
    session_id: u32,
    coordinate: RecordCoordinate,
    destination_file: String,
    target_index: usize,
    host: State<'_, EditorHost>,
) -> Result<ReorderRecordsOutcome, EditorError> {
    run_host_command(host, move |host| {
        host.sessions()
            .transfer_record(session_id, &coordinate, &destination_file, target_index)
    })
    .await
}

pub(crate) async fn run_blocking<T>(
    work: impl FnOnce() -> Result<T, EditorError> + Send + 'static,
) -> Result<T, EditorError>
where
    T: Send + 'static,
{
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| EditorError::other(format!("background command failed: {error}")))?
}

pub(crate) async fn run_host_command<T>(
    host: State<'_, EditorHost>,
    work: impl FnOnce(EditorHost) -> Result<T, EditorError> + Send + 'static,
) -> Result<T, EditorError>
where
    T: Send + 'static,
{
    // Tauri State 的借用不能进入阻塞线程，统一在边界处克隆线程安全的 host。
    let host = host.inner().clone();
    run_blocking(move || work(host)).await
}

fn open_with_default_application(path: &Path) -> Result<(), EditorError> {
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = std::process::Command::new("rundll32.exe");
        command.arg("url.dll,FileProtocolHandler").arg(path);
        command
    };
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = std::process::Command::new("open");
        command.arg(path);
        command
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = {
        let mut command = std::process::Command::new("xdg-open");
        command.arg(path);
        command
    };
    command.spawn().map(|_| ()).map_err(|error| {
        EditorError::other(format!("failed to open `{}`: {error}", path.display()))
    })
}
