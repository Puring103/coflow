//! Plugin tauri commands (thin adapter).

use super::manifest::{FrontendPluginBundle, FrontendPlugins, ProjectFrontendPlugins};
use super::store::{
    install_frontend_plugin_bundle, install_project_frontend_plugin_bundle,
    list_frontend_plugin_bundles, project_frontend_plugins, read_project_plugins,
    remove_project_frontend_plugin, uninstall_frontend_plugin_bundle,
    update_project_frontend_plugin_enabled,
};
use crate::commands::{run_blocking, run_host_command};
use cfd_editor_core::{editor::EditorError, EditorHost};
use std::path::PathBuf;
use tauri::{AppHandle, State};

#[tauri::command]
pub(crate) async fn install_frontend_plugin(
    manifest_path: String,
    app: AppHandle,
) -> Result<FrontendPluginBundle, EditorError> {
    run_blocking(move || {
        let manifest_path = PathBuf::from(manifest_path);
        install_frontend_plugin_bundle(&manifest_path, &app)
    })
    .await
}

#[tauri::command]
pub(crate) async fn list_frontend_plugins(app: AppHandle) -> Result<FrontendPlugins, EditorError> {
    run_blocking(move || list_frontend_plugin_bundles(&app)).await
}

#[tauri::command]
pub(crate) async fn uninstall_frontend_plugin(
    id: String,
    app: AppHandle,
) -> Result<(), EditorError> {
    run_blocking(move || uninstall_frontend_plugin_bundle(&id, &app)).await
}

#[tauri::command]
pub(crate) async fn install_project_frontend_plugin(
    session_id: u32,
    manifest_path: String,
    host: State<'_, EditorHost>,
) -> Result<FrontendPluginBundle, EditorError> {
    run_host_command(host, move |host| {
        let project_root = host.sessions().project_root_for(session_id)?;
        install_project_frontend_plugin_bundle(&project_root, &PathBuf::from(manifest_path))
    })
    .await
}

#[tauri::command]
pub(crate) async fn list_project_frontend_plugins(
    session_id: u32,
    host: State<'_, EditorHost>,
) -> Result<ProjectFrontendPlugins, EditorError> {
    run_host_command(host, move |host| {
        let project_root = host.sessions().project_root_for(session_id)?;
        let config = read_project_plugins(&project_root)?;
        Ok(project_frontend_plugins(&project_root, config))
    })
    .await
}

#[tauri::command]
pub(crate) async fn uninstall_project_frontend_plugin(
    session_id: u32,
    id: String,
    host: State<'_, EditorHost>,
) -> Result<(), EditorError> {
    run_host_command(host, move |host| {
        let project_root = host.sessions().project_root_for(session_id)?;
        remove_project_frontend_plugin(&project_root, &id)
    })
    .await
}

#[tauri::command]
pub(crate) async fn set_project_frontend_plugin_enabled(
    session_id: u32,
    id: String,
    enabled: bool,
    host: State<'_, EditorHost>,
) -> Result<(), EditorError> {
    run_host_command(host, move |host| {
        let project_root = host.sessions().project_root_for(session_id)?;
        update_project_frontend_plugin_enabled(&project_root, &id, enabled)
    })
    .await
}
