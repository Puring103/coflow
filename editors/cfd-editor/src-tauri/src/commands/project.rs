//! 项目生命周期、构建、设置与源码打开命令。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use cfd_editor_core::EditorHost;
use tauri::State;

use super::run_host_command;
use crate::open::open_path;
use cfd_editor_core::editor::{
    DimensionFileRecords, EditorError, EditorProjectSettings, EditorRecordGroup,
    EditorWorkspaceState, ProjectBootstrap, ViewConfig,
};
use coflow_runtime::ProjectDiff;

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
pub(crate) async fn set_graph_compact_mode(
    session_id: u32,
    view_key: String,
    compact: bool,
    host: State<'_, EditorHost>,
) -> Result<(), EditorError> {
    run_host_command(host, move |host| {
        host.sessions()
            .set_graph_compact_mode(session_id, view_key, compact)
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
        open_path(&path)
    })
    .await
}
