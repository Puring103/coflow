#![allow(clippy::multiple_crate_versions, clippy::unreachable)]

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

pub mod editor;
mod host;
mod watcher;

use coflow_data_model::{CfdPathSegment, CfdValue};
use coflow_extension_api::ExtensionManifest;
use coflow_runtime::{
    DimensionInfo, DimensionValueCoordinate, DimensionValueView, RecordCoordinate,
};
use editor::{
    BatchWriteFieldInput, BatchWriteFieldOutcome, CollectionEdit, CreateRecordDraft,
    DeleteRecordOutcome, DimensionFileRecords, EditorError, EditorProjectSettings,
    EditorRecordGroup, FileRecords, GraphData, GraphQuery, InsertRecordOutcome, ProjectSnapshot,
    RefTarget, RenameRecordOutcome, ReorderRecordsOutcome, WriteDimensionValueOutcome,
    WriteFieldOutcome,
};
use host::EditorHost;
use serde::Serialize;
use tauri::{AppHandle, Manager, State};

#[derive(Debug, Serialize)]
struct FrontendPluginBundle {
    manifest_path: String,
    id: String,
    name: String,
    description: String,
    version: String,
    source: String,
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
    })
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
async fn set_graph_enabled_fields(
    session_id: u32,
    file_path: String,
    actual_type: String,
    fields: Vec<String>,
    host: State<'_, EditorHost>,
) -> Result<EditorProjectSettings, EditorError> {
    let host = host.inner().clone();
    run_blocking(move || {
        host.sessions()
            .set_graph_enabled_fields(session_id, file_path, actual_type, fields)
    })
    .await
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
    destination_sheet: Option<String>,
    target_index: usize,
    host: State<'_, EditorHost>,
) -> Result<ReorderRecordsOutcome, EditorError> {
    let host = host.inner().clone();
    run_blocking(move || {
        host.sessions().transfer_record(
            session_id,
            &coordinate,
            &destination_file,
            destination_sheet.as_deref(),
            target_index,
        )
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
            let host = EditorHost::new().map_err(|err| err.to_string())?;
            app.manage(host);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            load_project,
            init_project,
            close_session,
            get_project_settings,
            get_project_dimensions,
            get_dimension_file_records,
            set_table_column_widths,
            set_record_groups,
            set_graph_enabled_fields,
            check_project,
            build_project,
            open_source_file,
            get_file_records,
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

    use super::load_frontend_plugin_bundle;

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
}
