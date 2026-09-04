#![allow(clippy::multiple_crate_versions, clippy::unreachable)]

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

mod extension_manifest;

/// Compatibility re-export for generated TypeScript binding tests and host consumers.
pub mod editor {
    pub use cfd_editor_core::editor::*;
}

use cfd_editor_core::{EditorEvent, EditorEventSink, EditorHost};
use coflow_runtime::{CfdPathSegment, CfdValue, FlatDiagnostic};
use coflow_runtime::{
    DimensionInfo, DimensionValueCoordinate, DimensionValueView, RecordCoordinate,
};
use editor::{
    BatchWriteFieldInput, BatchWriteFieldOutcome, CollectionEdit, CreateRecordDraft,
    DeleteRecordOutcome, DimensionFileRecords, EditorError, EditorProjectSettings,
    EditorRecordGroup, EditorWorkspaceState, FileRecords, GraphData, GraphQuery,
    InsertRecordOutcome, PluginSchemaType, ProjectBootstrap, ProjectSearchMode,
    ProjectSearchResults, RecordRow, RefTarget, RenameRecordOutcome, ReorderRecordsOutcome, ViewConfig,
    WriteDimensionValueOutcome, WriteFieldOutcome,
    FunctionDocumentState, LanguageCompletion, LanguageDocumentState, LanguageFormattingResult,
    LanguagePosition,
};
use extension_manifest::ExtensionManifest;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

const PROJECT_RELOADED_EVENT: &str = "project_reloaded";
const PROJECT_WATCH_ERROR_EVENT: &str = "project_watch_error";

#[derive(Debug, Clone)]
struct TauriEditorEventSink {
    app: AppHandle,
}

impl EditorEventSink for TauriEditorEventSink {
    fn emit(&self, event: EditorEvent) {
        match event {
            EditorEvent::ProjectReloaded(payload) => {
                let _ = self.app.emit(PROJECT_RELOADED_EVENT, payload);
            }
            EditorEvent::ProjectWatchError(payload) => {
                let _ = self.app.emit(PROJECT_WATCH_ERROR_EVENT, payload);
            }
        }
    }
}

const PROJECT_PLUGIN_DIR: &str = "editor-setting";
const PROJECT_PLUGIN_FILE: &str = "plugins.json";

#[derive(Debug, Clone, Serialize)]
struct FrontendPluginBundle {
    manifest_path: String,
    id: String,
    name: String,
    description: String,
    version: String,
    source: String,
    scope: PluginScope,
    enabled: bool,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum PluginScope {
    Global,
    Project,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct ProjectPluginsFile {
    #[serde(default = "project_plugin_file_version")]
    version: u32,
    #[serde(default)]
    plugins: Vec<ProjectPluginEntry>,
}

const fn project_plugin_file_version() -> u32 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProjectPluginEntry {
    id: String,
    manifest: String,
    #[serde(default = "default_plugin_enabled")]
    enabled: bool,
}

const fn default_plugin_enabled() -> bool {
    true
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
async fn install_frontend_plugin(
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
async fn list_frontend_plugins(app: AppHandle) -> Result<Vec<FrontendPluginBundle>, EditorError> {
    run_blocking(move || list_frontend_plugin_bundles(&app)).await
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
async fn uninstall_frontend_plugin(id: String, app: AppHandle) -> Result<(), EditorError> {
    run_blocking(move || uninstall_frontend_plugin_bundle(&id, &app)).await
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
async fn install_project_frontend_plugin(
    session_id: u32,
    manifest_path: String,
    host: State<'_, EditorHost>,
) -> Result<FrontendPluginBundle, EditorError> {
    let host = host.inner().clone();
    run_blocking(move || {
        let project_root = host.sessions().project_root_for(session_id)?;
        install_project_frontend_plugin_bundle(&project_root, &PathBuf::from(manifest_path))
    })
    .await
}

#[tauri::command]
async fn list_project_frontend_plugins(
    session_id: u32,
    host: State<'_, EditorHost>,
) -> Result<Vec<FrontendPluginBundle>, EditorError> {
    let host = host.inner().clone();
    run_blocking(move || {
        let project_root = host.sessions().project_root_for(session_id)?;
        list_project_frontend_plugin_bundles(&project_root)
    })
    .await
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
async fn uninstall_project_frontend_plugin(
    session_id: u32,
    id: String,
    host: State<'_, EditorHost>,
) -> Result<(), EditorError> {
    let host = host.inner().clone();
    run_blocking(move || {
        let project_root = host.sessions().project_root_for(session_id)?;
        remove_project_frontend_plugin(&project_root, &id)
    })
    .await
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
async fn set_project_frontend_plugin_enabled(
    session_id: u32,
    id: String,
    enabled: bool,
    host: State<'_, EditorHost>,
) -> Result<(), EditorError> {
    let host = host.inner().clone();
    run_blocking(move || {
        let project_root = host.sessions().project_root_for(session_id)?;
        update_project_frontend_plugin_enabled(&project_root, &id, enabled)
    })
    .await
}

fn load_frontend_plugin_bundle(manifest_path: &Path) -> Result<FrontendPluginBundle, EditorError> {
    if manifest_path
        .extension()
        .is_none_or(|extension| extension != "json")
    {
        return Err(EditorError::other("plugin manifest must be a .json file"));
    }
    let manifest_path = std::fs::canonicalize(manifest_path)
        .map_err(|error| EditorError::other(format!("failed to read plugin manifest: {error}")))?;
    let manifest_text = std::fs::read_to_string(&manifest_path)
        .map_err(|error| EditorError::other(format!("failed to read plugin manifest: {error}")))?;
    let manifest: ExtensionManifest = serde_json::from_str(&manifest_text)
        .map_err(|error| EditorError::other(format!("invalid plugin manifest: {error}")))?;
    if manifest.id.trim().is_empty()
        || manifest.name.trim().is_empty()
        || manifest.entry.trim().is_empty()
    {
        return Err(EditorError::other(
            "plugin manifest requires non-empty id, name, and entry",
        ));
    }
    let entry = PathBuf::from(&manifest.entry);
    if entry.is_absolute()
        || entry.components().any(|part| {
            matches!(
                part,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(EditorError::other(
            "plugin entry must be a relative path inside the plugin directory",
        ));
    }
    let plugin_dir = manifest_path
        .parent()
        .ok_or_else(|| EditorError::other("plugin manifest has no parent directory"))?;
    let entry_path = std::fs::canonicalize(plugin_dir.join(entry))
        .map_err(|error| EditorError::other(format!("failed to read plugin entry: {error}")))?;
    if !entry_path.starts_with(plugin_dir)
        || entry_path
            .extension()
            .is_none_or(|extension| extension != "js")
    {
        return Err(EditorError::other(
            "plugin entry must be a .js file inside the plugin directory",
        ));
    }
    let source = std::fs::read_to_string(entry_path)
        .map_err(|error| EditorError::other(format!("failed to read plugin bundle: {error}")))?;
    Ok(FrontendPluginBundle {
        manifest_path: manifest_path.display().to_string(),
        id: manifest.id,
        name: manifest.name,
        description: manifest.description,
        version: manifest.version,
        source,
        scope: PluginScope::Global,
        enabled: true,
    })
}

fn project_plugins_path(project_root: &Path) -> PathBuf {
    project_root
        .join(PROJECT_PLUGIN_DIR)
        .join(PROJECT_PLUGIN_FILE)
}

fn read_project_plugins(project_root: &Path) -> Result<ProjectPluginsFile, EditorError> {
    let path = project_plugins_path(project_root);
    if !path.exists() {
        return Ok(ProjectPluginsFile {
            version: project_plugin_file_version(),
            ..ProjectPluginsFile::default()
        });
    }
    let contents = std::fs::read_to_string(&path).map_err(|error| {
        EditorError::other(format!("failed to read {}: {error}", path.display()))
    })?;
    serde_json::from_str(&contents)
        .map_err(|error| EditorError::other(format!("failed to parse {}: {error}", path.display())))
}

fn write_project_plugins(
    project_root: &Path,
    plugins: &ProjectPluginsFile,
) -> Result<(), EditorError> {
    let path = project_plugins_path(project_root);
    let parent = path
        .parent()
        .ok_or_else(|| EditorError::other("project plugin path has no parent"))?;
    std::fs::create_dir_all(parent).map_err(|error| {
        EditorError::other(format!("failed to create {}: {error}", parent.display()))
    })?;
    let contents = serde_json::to_string_pretty(plugins).map_err(|error| {
        EditorError::other(format!("failed to encode project plugins: {error}"))
    })?;
    std::fs::write(&path, contents)
        .map_err(|error| EditorError::other(format!("failed to write {}: {error}", path.display())))
}

fn relative_project_path(project_root: &Path, target: &Path) -> Result<PathBuf, EditorError> {
    let root = std::fs::canonicalize(project_root)
        .map_err(|error| EditorError::other(format!("failed to resolve project root: {error}")))?;
    let target = std::fs::canonicalize(target).map_err(|error| {
        EditorError::other(format!("failed to resolve plugin manifest: {error}"))
    })?;
    let root_parts = root.components().collect::<Vec<_>>();
    let target_parts = target.components().collect::<Vec<_>>();
    let shared = root_parts
        .iter()
        .zip(&target_parts)
        .take_while(|(left, right)| left == right)
        .count();
    if shared == 0 {
        return Err(EditorError::other(
            "project and plugin manifest must be on the same volume",
        ));
    }
    let mut relative = PathBuf::new();
    for _ in shared..root_parts.len() {
        relative.push("..");
    }
    for component in &target_parts[shared..] {
        relative.push(component.as_os_str());
    }
    Ok(relative)
}

fn resolve_project_manifest(project_root: &Path, manifest: &str) -> Result<PathBuf, EditorError> {
    let path = Path::new(manifest);
    if path.is_absolute() {
        return Err(EditorError::other(
            "project plugin manifest must use a relative path",
        ));
    }
    std::fs::canonicalize(project_root.join(path)).map_err(|error| {
        EditorError::other(format!(
            "failed to resolve project plugin manifest `{manifest}`: {error}"
        ))
    })
}

fn project_bundle(
    project_root: &Path,
    entry: &ProjectPluginEntry,
) -> Result<FrontendPluginBundle, EditorError> {
    let manifest = resolve_project_manifest(project_root, &entry.manifest)?;
    let mut bundle = load_frontend_plugin_bundle(&manifest)?;
    if bundle.id != entry.id {
        return Err(EditorError::other(format!(
            "project plugin `{}` does not match manifest id `{}`",
            entry.id, bundle.id
        )));
    }
    bundle.scope = PluginScope::Project;
    bundle.enabled = entry.enabled;
    Ok(bundle)
}

fn install_project_frontend_plugin_bundle(
    project_root: &Path,
    manifest: &Path,
) -> Result<FrontendPluginBundle, EditorError> {
    let mut bundle = load_frontend_plugin_bundle(manifest)?;
    if !valid_plugin_id(&bundle.id) {
        return Err(EditorError::other(
            "plugin id may only contain ASCII letters, digits, hyphens, and underscores",
        ));
    }
    let relative = relative_project_path(project_root, manifest)?;
    let mut config = read_project_plugins(project_root)?;
    config.version = project_plugin_file_version();
    config.plugins.retain(|entry| entry.id != bundle.id);
    config.plugins.push(ProjectPluginEntry {
        id: bundle.id.clone(),
        manifest: relative.to_string_lossy().replace('\\', "/"),
        enabled: true,
    });
    config.plugins.sort_by(|left, right| left.id.cmp(&right.id));
    write_project_plugins(project_root, &config)?;
    bundle.scope = PluginScope::Project;
    bundle.enabled = true;
    Ok(bundle)
}

fn list_project_frontend_plugin_bundles(
    project_root: &Path,
) -> Result<Vec<FrontendPluginBundle>, EditorError> {
    let config = read_project_plugins(project_root)?;
    let mut bundles = config
        .plugins
        .iter()
        .map(|entry| project_bundle(project_root, entry))
        .collect::<Result<Vec<_>, _>>()?;
    bundles.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(bundles)
}

fn remove_project_frontend_plugin(project_root: &Path, id: &str) -> Result<(), EditorError> {
    let mut config = read_project_plugins(project_root)?;
    let before = config.plugins.len();
    config.plugins.retain(|entry| entry.id != id);
    if config.plugins.len() == before {
        return Err(EditorError::not_found(format!(
            "project plugin `{id}` not found"
        )));
    }
    write_project_plugins(project_root, &config)
}

fn update_project_frontend_plugin_enabled(
    project_root: &Path,
    id: &str,
    enabled: bool,
) -> Result<(), EditorError> {
    let mut config = read_project_plugins(project_root)?;
    let entry = config
        .plugins
        .iter_mut()
        .find(|entry| entry.id == id)
        .ok_or_else(|| EditorError::not_found(format!("project plugin `{id}` not found")))?;
    entry.enabled = enabled;
    write_project_plugins(project_root, &config)
}

fn plugin_data_dir(app: &AppHandle) -> Result<PathBuf, EditorError> {
    app.path()
        .app_data_dir()
        .map(|path| path.join("plugins"))
        .map_err(|error| {
            EditorError::other(format!("failed to resolve plugin data directory: {error}"))
        })
}

fn valid_plugin_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
}

fn install_frontend_plugin_bundle(
    manifest_path: &Path,
    app: &AppHandle,
) -> Result<FrontendPluginBundle, EditorError> {
    let bundle = load_frontend_plugin_bundle(manifest_path)?;
    if !valid_plugin_id(&bundle.id) {
        return Err(EditorError::other(
            "plugin id may only contain ASCII letters, digits, hyphens, and underscores",
        ));
    }
    let plugin_dir = plugin_data_dir(app)?.join(&bundle.id);
    std::fs::create_dir_all(&plugin_dir).map_err(|error| {
        EditorError::other(format!("failed to create plugin data directory: {error}"))
    })?;
    let manifest = ExtensionManifest {
        id: bundle.id,
        name: bundle.name,
        description: bundle.description,
        version: bundle.version,
        entry: "plugin.js".to_string(),
    };
    let manifest_text = serde_json::to_string_pretty(&manifest).map_err(|error| {
        EditorError::other(format!(
            "failed to serialize installed plugin manifest: {error}"
        ))
    })?;
    std::fs::write(plugin_dir.join("plugin.json"), manifest_text).map_err(|error| {
        EditorError::other(format!("failed to install plugin manifest: {error}"))
    })?;
    std::fs::write(plugin_dir.join("plugin.js"), bundle.source)
        .map_err(|error| EditorError::other(format!("failed to install plugin bundle: {error}")))?;
    load_frontend_plugin_bundle(&plugin_dir.join("plugin.json"))
}

fn list_frontend_plugin_bundles(app: &AppHandle) -> Result<Vec<FrontendPluginBundle>, EditorError> {
    let root = plugin_data_dir(app)?;
    if !root.exists() {
        return Ok(Vec::new());
    }
    let entries = std::fs::read_dir(root).map_err(|error| {
        EditorError::other(format!("failed to list installed plugins: {error}"))
    })?;
    let mut bundles = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path().join("plugin.json"))
        .filter(|manifest| manifest.is_file())
        .map(|manifest| load_frontend_plugin_bundle(&manifest))
        .collect::<Result<Vec<_>, _>>()?;
    bundles.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(bundles)
}

fn uninstall_frontend_plugin_bundle(id: &str, app: &AppHandle) -> Result<(), EditorError> {
    if !valid_plugin_id(id) {
        return Err(EditorError::other("invalid plugin id"));
    }
    let path = plugin_data_dir(app)?.join(id);
    if path.exists() {
        std::fs::remove_dir_all(path)
            .map_err(|error| EditorError::other(format!("failed to uninstall plugin: {error}")))?;
    }
    Ok(())
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
async fn load_project(
    yaml_path: String,
    host: State<'_, EditorHost>,
) -> Result<ProjectBootstrap, EditorError> {
    let host = host.inner().clone();
    run_blocking(move || host.load_project(&PathBuf::from(yaml_path))).await
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
async fn init_project(
    dir: String,
    host: State<'_, EditorHost>,
) -> Result<ProjectBootstrap, EditorError> {
    let host = host.inner().clone();
    run_blocking(move || host.init_project(&PathBuf::from(dir))).await
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
async fn close_session(session_id: u32, host: State<'_, EditorHost>) -> Result<(), EditorError> {
    let host = host.inner().clone();
    run_blocking(move || host.close_session(session_id)).await
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
async fn reload_session(
    session_id: u32,
    host: State<'_, EditorHost>,
) -> Result<ProjectBootstrap, EditorError> {
    let host = host.inner().clone();
    run_blocking(move || host.reload_session(session_id)).await
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
async fn add_project_input(
    session_id: u32,
    kind: String,
    path: String,
    host: State<'_, EditorHost>,
) -> Result<ProjectBootstrap, EditorError> {
    let kind = match kind.as_str() {
        "schema" => coflow_runtime::ProjectInputKind::Schema,
        "data" => coflow_runtime::ProjectInputKind::Data,
        _ => return Err(EditorError::other("project input kind must be schema or data")),
    };
    let host = host.inner().clone();
    run_blocking(move || host.sessions().add_project_input(session_id, kind, &PathBuf::from(path))).await
}

fn project_input_kind(kind: &str) -> Result<coflow_runtime::ProjectInputKind, EditorError> {
    match kind {
        "schema" => Ok(coflow_runtime::ProjectInputKind::Schema),
        "data" => Ok(coflow_runtime::ProjectInputKind::Data),
        _ => Err(EditorError::other("project input kind must be schema or data")),
    }
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
async fn create_project_file(session_id: u32, kind: String, parent_path: String, file_name: String, host: State<'_, EditorHost>) -> Result<ProjectBootstrap, EditorError> {
    let kind = project_input_kind(&kind)?;
    let host = host.inner().clone();
    run_blocking(move || host.sessions().create_project_file(session_id, kind, Path::new(&parent_path), &file_name)).await
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
async fn delete_project_entry(session_id: u32, path: String, host: State<'_, EditorHost>) -> Result<ProjectBootstrap, EditorError> {
    let host = host.inner().clone();
    run_blocking(move || host.sessions().delete_project_entry(session_id, Path::new(&path))).await
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
async fn get_project_dimensions(
    session_id: u32,
    host: State<'_, EditorHost>,
) -> Result<Vec<DimensionInfo>, EditorError> {
    let host = host.inner().clone();
    run_blocking(move || host.sessions().get_project_dimensions(session_id)).await
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
async fn get_dimension_file_records(
    session_id: u32,
    file_path: String,
    host: State<'_, EditorHost>,
) -> Result<DimensionFileRecords, EditorError> {
    let host = host.inner().clone();
    run_blocking(move || {
        host.sessions()
            .get_dimension_file_records(session_id, &file_path)
    })
    .await
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
async fn set_default_table_column_widths(
    session_id: u32,
    file_path: String,
    actual_type: String,
    widths: BTreeMap<String, f64>,
    host: State<'_, EditorHost>,
) -> Result<EditorProjectSettings, EditorError> {
    let host = host.inner().clone();
    run_blocking(move || {
        host.sessions()
            .set_default_table_column_widths(session_id, file_path, actual_type, widths)
    })
    .await
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
async fn set_views(
    session_id: u32,
    file_path: String,
    actual_type: String,
    views: Vec<ViewConfig>,
    host: State<'_, EditorHost>,
) -> Result<EditorProjectSettings, EditorError> {
    let host = host.inner().clone();
    run_blocking(move || {
        host.sessions()
            .set_views(session_id, file_path, actual_type, views)
    })
    .await
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
async fn set_view_column_widths(
    session_id: u32,
    file_path: String,
    actual_type: String,
    view_id: String,
    widths: BTreeMap<String, f64>,
    host: State<'_, EditorHost>,
) -> Result<EditorProjectSettings, EditorError> {
    let host = host.inner().clone();
    run_blocking(move || {
        host.sessions()
            .set_view_column_widths(session_id, file_path, actual_type, view_id, widths)
    })
    .await
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
async fn set_record_groups(
    session_id: u32,
    file_path: String,
    actual_type: String,
    groups: Vec<EditorRecordGroup>,
    host: State<'_, EditorHost>,
) -> Result<EditorProjectSettings, EditorError> {
    let host = host.inner().clone();
    run_blocking(move || {
        host.sessions()
            .set_record_groups(session_id, file_path, actual_type, groups)
    })
    .await
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
async fn set_workspace(
    session_id: u32,
    workspace: EditorWorkspaceState,
    host: State<'_, EditorHost>,
) -> Result<EditorProjectSettings, EditorError> {
    let host = host.inner().clone();
    run_blocking(move || host.sessions().set_workspace(session_id, workspace)).await
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
async fn check_project(
    session_id: u32,
    host: State<'_, EditorHost>,
) -> Result<String, EditorError> {
    let host = host.inner().clone();
    run_blocking(move || host.sessions().check_project(session_id)).await
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
async fn build_project(
    session_id: u32,
    host: State<'_, EditorHost>,
) -> Result<String, EditorError> {
    let host = host.inner().clone();
    run_blocking(move || host.sessions().build_project(session_id)).await
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
async fn build_project_status(
    session_id: u32,
    host: State<'_, EditorHost>,
) -> Result<bool, EditorError> {
    let host = host.inner().clone();
    run_blocking(move || host.sessions().build_project_status(session_id)).await
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
async fn open_source_file(
    session_id: u32,
    file_path: String,
    host: State<'_, EditorHost>,
) -> Result<(), EditorError> {
    let host = host.inner().clone();
    run_blocking(move || {
        let path = host.sessions().source_file_path(session_id, &file_path)?;
        open_with_default_application(&path)
    })
    .await
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
async fn read_source_text(
    session_id: u32,
    file_path: String,
    host: State<'_, EditorHost>,
) -> Result<String, EditorError> {
    let host = host.inner().clone();
    run_blocking(move || host.sessions().read_source_text(session_id, &file_path)).await
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
async fn sync_language_document(
    session_id: u32,
    file_path: String,
    source: String,
    version: i64,
    host: State<'_, EditorHost>,
) -> Result<LanguageDocumentState, EditorError> {
    let host = host.inner().clone();
    run_blocking(move || {
        host.sessions()
            .sync_language_document(session_id, &file_path, &source, version)
    })
    .await
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
async fn validate_source_text(
    session_id: u32,
    file_path: String,
    source: String,
    host: State<'_, EditorHost>,
) -> Result<Vec<FlatDiagnostic>, EditorError> {
    let host = host.inner().clone();
    run_blocking(move || {
        host.sessions()
            .validate_source_text(session_id, &file_path, &source)
    })
    .await
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
async fn complete_language_document(
    session_id: u32,
    file_path: String,
    source: String,
    version: i64,
    position: LanguagePosition,
    host: State<'_, EditorHost>,
) -> Result<Vec<LanguageCompletion>, EditorError> {
    let host = host.inner().clone();
    run_blocking(move || {
        host.sessions().complete_language_document(
            session_id,
            &file_path,
            &source,
            version,
            &position,
        )
    })
    .await
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
async fn format_language_document(
    session_id: u32,
    file_path: String,
    source: String,
    version: i64,
    host: State<'_, EditorHost>,
) -> Result<LanguageFormattingResult, EditorError> {
    let host = host.inner().clone();
    run_blocking(move || {
        host.sessions()
            .format_language_document(session_id, &file_path, &source, version)
    })
    .await
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
async fn close_language_document(
    session_id: u32,
    file_path: String,
    host: State<'_, EditorHost>,
) -> Result<(), EditorError> {
    let host = host.inner().clone();
    run_blocking(move || host.sessions().close_language_document(session_id, &file_path)).await
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
async fn function_document(
    session_id: u32,
    source: String,
    body: Option<String>,
    host: State<'_, EditorHost>,
) -> Result<FunctionDocumentState, EditorError> {
    let host = host.inner().clone();
    run_blocking(move || host.sessions().function_document(session_id, &source, body.as_deref()))
        .await
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
async fn write_source_text(
    session_id: u32,
    file_path: String,
    source: String,
    host: State<'_, EditorHost>,
) -> Result<ProjectBootstrap, EditorError> {
    let host = host.inner().clone();
    run_blocking(move || {
        host.sessions()
            .write_source_text(session_id, &file_path, &source)
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
async fn search_records(
    session_id: u32,
    query: String,
    mode: ProjectSearchMode,
    limit: usize,
    host: State<'_, EditorHost>,
) -> Result<ProjectSearchResults, EditorError> {
    let host = host.inner().clone();
    run_blocking(move || {
        host.sessions()
            .search_records(session_id, &query, mode, limit)
    })
    .await
}

#[tauri::command]
async fn get_plugin_schema(
    session_id: u32,
    host: State<'_, EditorHost>,
) -> Result<Vec<PluginSchemaType>, EditorError> {
    let host = host.inner().clone();
    run_blocking(move || host.sessions().get_plugin_schema(session_id)).await
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
async fn get_plugin_records_by_type(
    session_id: u32,
    type_name: String,
    host: State<'_, EditorHost>,
) -> Result<Vec<RecordRow>, EditorError> {
    let host = host.inner().clone();
    run_blocking(move || {
        host.sessions()
            .get_plugin_records_by_type(session_id, &type_name)
    })
    .await
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
) -> Result<Vec<editor::types::EnumVariantOption>, EditorError> {
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
async fn write_fields(
    session_id: u32,
    writes: Vec<BatchWriteFieldInput>,
    host: State<'_, EditorHost>,
) -> Result<BatchWriteFieldOutcome, EditorError> {
    let host = host.inner().clone();
    run_blocking(move || host.sessions().write_fields(session_id, &writes)).await
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

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
async fn swap_records(
    session_id: u32,
    first: RecordCoordinate,
    second: RecordCoordinate,
    host: State<'_, EditorHost>,
) -> Result<ReorderRecordsOutcome, EditorError> {
    let host = host.inner().clone();
    run_blocking(move || host.sessions().swap_records(session_id, &first, &second)).await
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
async fn move_record(
    session_id: u32,
    coordinate: RecordCoordinate,
    target_index: usize,
    host: State<'_, EditorHost>,
) -> Result<ReorderRecordsOutcome, EditorError> {
    let host = host.inner().clone();
    run_blocking(move || {
        host.sessions()
            .move_record(session_id, &coordinate, target_index)
    })
    .await
}

#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
async fn transfer_record(
    session_id: u32,
    coordinate: RecordCoordinate,
    destination_file: String,
    target_index: usize,
    host: State<'_, EditorHost>,
) -> Result<ReorderRecordsOutcome, EditorError> {
    let host = host.inner().clone();
    run_blocking(move || {
        host.sessions()
            .transfer_record(session_id, &coordinate, &destination_file, target_index)
    })
    .await
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
            let events = Arc::new(TauriEditorEventSink {
                app: app.handle().clone(),
            });
            let host = EditorHost::new(events).map_err(|err| err.to_string())?;
            app.manage(host);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            load_project,
            init_project,
            close_session,
            reload_session,
            add_project_input,
            create_project_file,
            delete_project_entry,
            get_project_settings,
            get_project_dimensions,
            get_dimension_file_records,
            set_default_table_column_widths,
            set_views,
            set_view_column_widths,
            set_record_groups,
            set_workspace,
            check_project,
            build_project,
            build_project_status,
            open_source_file,
            read_source_text,
            sync_language_document,
            validate_source_text,
            complete_language_document,
            format_language_document,
            close_language_document,
            function_document,
            write_source_text,
            get_file_records,
            search_records,
            get_plugin_schema,
            get_plugin_records_by_type,
            get_graph,
            get_enum_variants,
            get_ref_targets,
            make_default_object,
            create_record_draft,
            render_cell_text,
            parse_cell_text,
            write_field,
            write_fields,
            get_dimension_value,
            write_dimension_value,
            edit_collection,
            insert_record,
            rename_record_key,
            delete_record,
            swap_records,
            move_record,
            transfer_record,
            install_frontend_plugin,
            list_frontend_plugins,
            uninstall_frontend_plugin,
            install_project_frontend_plugin,
            list_project_frontend_plugins,
            uninstall_project_frontend_plugin,
            set_project_frontend_plugin_enabled,
        ])
        .run(tauri::generate_context!())
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

#[cfg(test)]
#[allow(clippy::expect_used)]
mod frontend_plugin_tests {
    use std::fs;

    use super::{
        install_project_frontend_plugin_bundle, list_project_frontend_plugin_bundles,
        load_frontend_plugin_bundle, resolve_project_manifest,
    };

    fn temp_plugin_dir(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "coflow-editor-plugin-{name}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock should be after epoch")
                .as_nanos()
        ))
    }

    #[test]
    fn loads_a_local_frontend_plugin_bundle() {
        let dir = temp_plugin_dir("valid");
        fs::create_dir_all(dir.join("dist")).expect("create plugin directory");
        let manifest = dir.join("plugin.json");
        fs::write(
            &manifest,
            r#"{"id":"sample","name":"Sample","entry":"dist/plugin.js"}"#,
        )
        .expect("write manifest");
        fs::write(
            dir.join("dist/plugin.js"),
            "window.CfdEditorPlugins.register({ id: 'sample' })",
        )
        .expect("write bundle");

        let bundle = load_frontend_plugin_bundle(&manifest).expect("load plugin");
        assert_eq!(bundle.id, "sample");
        assert!(bundle.source.contains("register"));
        fs::remove_dir_all(dir).expect("remove plugin directory");
    }

    #[test]
    fn rejects_plugin_entry_outside_the_manifest_directory() {
        let dir = temp_plugin_dir("traversal");
        fs::create_dir_all(&dir).expect("create plugin directory");
        let manifest = dir.join("plugin.json");
        fs::write(
            &manifest,
            r#"{"id":"sample","name":"Sample","entry":"../plugin.js"}"#,
        )
        .expect("write manifest");

        let error = load_frontend_plugin_bundle(&manifest).expect_err("reject traversal entry");
        assert!(error.message.contains("relative path"));
        fs::remove_dir_all(dir).expect("remove plugin directory");
    }

    #[test]
    fn project_plugins_store_external_manifests_as_relative_paths() {
        let root = temp_plugin_dir("project-root");
        let plugin_dir = root
            .parent()
            .expect("temp root parent")
            .join("shared-plugin");
        fs::create_dir_all(&root).expect("create project root");
        fs::create_dir_all(plugin_dir.join("dist")).expect("create plugin directory");
        let manifest = plugin_dir.join("plugin.json");
        fs::write(
            &manifest,
            r#"{"id":"shared","name":"Shared","entry":"dist/plugin.js"}"#,
        )
        .expect("write manifest");
        fs::write(plugin_dir.join("dist/plugin.js"), "export default () => {}")
            .expect("write bundle");

        let bundle = install_project_frontend_plugin_bundle(&root, &manifest)
            .expect("install project plugin");
        assert_eq!(bundle.id, "shared");
        assert!(matches!(bundle.scope, super::PluginScope::Project));
        let config = fs::read_to_string(root.join("editor-setting/plugins.json"))
            .expect("read project config");
        assert!(config.contains("../shared-plugin/plugin.json"));
        assert!(!config.contains(plugin_dir.to_string_lossy().as_ref()));
        assert_eq!(
            list_project_frontend_plugin_bundles(&root)
                .expect("list project plugins")
                .len(),
            1
        );
        fs::remove_dir_all(root).expect("remove project root");
        fs::remove_dir_all(plugin_dir).expect("remove plugin directory");
    }

    #[test]
    fn project_plugin_config_rejects_absolute_manifest_paths() {
        let root = temp_plugin_dir("absolute-path");
        fs::create_dir_all(root.join("editor-setting")).expect("create project root");
        let absolute = root.join("external/plugin.json");
        let error = resolve_project_manifest(&root, absolute.to_string_lossy().as_ref())
            .expect_err("reject absolute manifest path");
        assert!(error.message.contains("relative path"));
        fs::remove_dir_all(root).expect("remove project root");
    }
}
