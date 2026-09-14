//! Tauri 前端插件的命令、持久化与路径安全边界。

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use cfd_editor_core::EditorHost;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};

use crate::commands::{run_blocking, run_host_command};
use crate::editor::EditorError;
use crate::plugin_manifest::PluginManifest;

const PROJECT_PLUGIN_DIR: &str = "editor-setting";
const PROJECT_PLUGIN_FILE: &str = "plugins.json";

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
pub struct FrontendPluginBundle {
    manifest_path: String,
    id: String,
    name: String,
    description: String,
    version: String,
    source: String,
    scope: PluginScope,
    enabled: bool,
}

#[derive(Debug, Default, Serialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        rename = "FrontendPluginState",
        export,
        export_to = "../../frontend/src/bindings/"
    )
)]
pub struct FrontendPlugins {
    plugins: Vec<FrontendPluginBundle>,
    errors: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
#[serde(rename_all = "snake_case")]
pub enum PluginScope {
    Global,
    Project,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct ProjectPluginsFile {
    #[serde(default = "project_plugin_file_version")]
    version: u32,
    #[serde(default)]
    plugins: Vec<ProjectPluginEntry>,
    #[serde(default)]
    defaults: ProjectPluginDefaults,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
pub struct ProjectPluginDefaults {
    #[serde(default)]
    views: BTreeMap<String, String>,
    #[serde(default)]
    presentations: BTreeMap<String, BTreeMap<String, String>>,
}

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        rename = "FrontendPluginProjectState",
        export,
        export_to = "../../frontend/src/bindings/"
    )
)]
pub struct ProjectFrontendPlugins {
    plugins: Vec<FrontendPluginBundle>,
    defaults: ProjectPluginDefaults,
    errors: Vec<String>,
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

fn load_frontend_plugin_bundle(manifest_path: &Path) -> Result<FrontendPluginBundle, EditorError> {
    if manifest_path
        .extension()
        .is_none_or(|extension| extension != "json")
    {
        return Err(EditorError::other("plugin manifest must be a .json file"));
    }
    let manifest_path = coflow_runtime::canonicalize_path(manifest_path)
        .map_err(|error| EditorError::other(format!("failed to read plugin manifest: {error}")))?;
    let manifest_text = std::fs::read_to_string(&manifest_path)
        .map_err(|error| EditorError::other(format!("failed to read plugin manifest: {error}")))?;
    let manifest: PluginManifest = serde_json::from_str(&manifest_text)
        .map_err(|error| EditorError::other(format!("invalid plugin manifest: {error}")))?;
    if !valid_plugin_id(&manifest.id) {
        return Err(EditorError::other(
            "plugin id may only contain ASCII letters, digits, hyphens, and underscores",
        ));
    }
    if manifest.name.trim().is_empty() || manifest.entry.trim().is_empty() {
        return Err(EditorError::other(
            "plugin manifest requires non-empty name and entry",
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
    let entry_path = coflow_runtime::canonicalize_path(plugin_dir.join(entry))
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
    let root = coflow_runtime::canonicalize_path(project_root)
        .map_err(|error| EditorError::other(format!("failed to resolve project root: {error}")))?;
    let target = coflow_runtime::canonicalize_path(target).map_err(|error| {
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
    coflow_runtime::canonicalize_path(project_root.join(path)).map_err(|error| {
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
    let relative = relative_project_path(project_root, manifest)?;
    let mut config = read_project_plugins(project_root)?;
    config.version = project_plugin_file_version();
    config.plugins.retain(|entry| entry.id != bundle.id);
    config.plugins.push(ProjectPluginEntry {
        id: bundle.id.clone(),
        manifest: relative.to_string_lossy().replace('\\', "/"),
        enabled: true,
    });
    write_project_plugins(project_root, &config)?;
    bundle.scope = PluginScope::Project;
    bundle.enabled = true;
    Ok(bundle)
}

fn project_frontend_plugins(
    project_root: &Path,
    config: ProjectPluginsFile,
) -> ProjectFrontendPlugins {
    let mut plugins = Vec::new();
    let mut errors = Vec::new();
    // 单个插件清单失效不得阻断同一项目中的其他插件。
    for entry in &config.plugins {
        match project_bundle(project_root, entry) {
            Ok(bundle) => plugins.push(bundle),
            Err(error) => errors.push(format!("{}: {}", entry.id, error.message)),
        }
    }
    ProjectFrontendPlugins {
        plugins,
        defaults: config.defaults,
        errors,
    }
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
    let plugin_dir = plugin_data_dir(app)?.join(&bundle.id);
    std::fs::create_dir_all(&plugin_dir).map_err(|error| {
        EditorError::other(format!("failed to create plugin data directory: {error}"))
    })?;
    let manifest = PluginManifest {
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

fn list_frontend_plugin_bundles(app: &AppHandle) -> Result<FrontendPlugins, EditorError> {
    let root = plugin_data_dir(app)?;
    if !root.exists() {
        return Ok(FrontendPlugins::default());
    }
    let entries = std::fs::read_dir(root).map_err(|error| {
        EditorError::other(format!("failed to list installed plugins: {error}"))
    })?;
    let mut manifests = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path().join("plugin.json"))
        .filter(|manifest| manifest.is_file())
        .collect::<Vec<_>>();
    manifests.sort();
    let mut plugins = Vec::new();
    let mut errors = Vec::new();
    for manifest in manifests {
        match load_frontend_plugin_bundle(&manifest) {
            Ok(bundle) => plugins.push(bundle),
            Err(error) => errors.push(format!("{}: {}", manifest.display(), error.message)),
        }
    }
    plugins.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(FrontendPlugins { plugins, errors })
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

#[cfg(test)]
#[allow(clippy::expect_used)]
mod frontend_plugin_tests {
    use std::fs;

    use super::{
        install_project_frontend_plugin_bundle, load_frontend_plugin_bundle,
        project_frontend_plugins, read_project_plugins, resolve_project_manifest,
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
            "export default function activate(host) { host.register.openPage('sample') }",
        )
        .expect("write bundle");

        let bundle = load_frontend_plugin_bundle(&manifest).expect("load plugin");
        assert_eq!(bundle.id, "sample");
        assert!(bundle.source.contains("activate"));
        fs::remove_dir_all(dir).expect("remove plugin directory");
    }

    #[test]
    fn rejects_invalid_plugin_ids_when_loading_manifests() {
        let dir = temp_plugin_dir("invalid-id");
        fs::create_dir_all(&dir).expect("create plugin directory");
        let manifest = dir.join("plugin.json");
        fs::write(
            &manifest,
            r#"{"id":"invalid/id","name":"Invalid","entry":"plugin.js"}"#,
        )
        .expect("write manifest");
        fs::write(dir.join("plugin.js"), "export default () => {}").expect("write bundle");

        let error = load_frontend_plugin_bundle(&manifest).expect_err("reject invalid id");

        assert!(error.message.contains("plugin id"));
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
        let state = project_frontend_plugins(
            &root,
            read_project_plugins(&root).expect("read project plugins"),
        );
        assert_eq!(state.plugins.len(), 1);
        assert!(state.errors.is_empty());
        fs::remove_dir_all(root).expect("remove project root");
        fs::remove_dir_all(plugin_dir).expect("remove plugin directory");
    }

    #[test]
    fn project_plugin_loading_isolates_invalid_entries() {
        let root = temp_plugin_dir("isolated-load");
        fs::create_dir_all(root.join("editor-setting")).expect("create project settings");
        fs::create_dir_all(root.join("valid")).expect("create valid plugin");
        fs::write(
            root.join("valid/plugin.json"),
            r#"{"id":"valid","name":"Valid","entry":"plugin.js"}"#,
        )
        .expect("write valid manifest");
        fs::write(root.join("valid/plugin.js"), "export default () => {}")
            .expect("write valid bundle");
        fs::write(
            root.join("editor-setting/plugins.json"),
            r#"{
                "version": 1,
                "plugins": [
                    { "id": "missing", "manifest": "missing/plugin.json" },
                    { "id": "valid", "manifest": "valid/plugin.json" }
                ]
            }"#,
        )
        .expect("write project plugins");

        let state = project_frontend_plugins(
            &root,
            read_project_plugins(&root).expect("read project plugins"),
        );

        assert_eq!(state.plugins.len(), 1);
        assert_eq!(state.plugins[0].id, "valid");
        assert_eq!(state.errors.len(), 1);
        assert!(state.errors[0].contains("missing"));
        fs::remove_dir_all(root).expect("remove project root");
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

    #[test]
    fn project_plugin_config_reads_default_contributions() {
        let root = temp_plugin_dir("defaults");
        fs::create_dir_all(root.join("editor-setting")).expect("create project settings");
        fs::write(
            root.join("editor-setting/plugins.json"),
            r#"{
                "version": 1,
                "plugins": [],
                "defaults": {
                    "views": { "Foo": "analysis/table" },
                    "presentations": { "Bar": { "cell": "format/cell" } }
                }
            }"#,
        )
        .expect("write plugin defaults");

        let config = read_project_plugins(&root).expect("read project plugin config");
        assert_eq!(
            config.defaults.views.get("Foo").map(String::as_str),
            Some("analysis/table")
        );
        assert_eq!(
            config
                .defaults
                .presentations
                .get("Bar")
                .and_then(|slots| slots.get("cell"))
                .map(String::as_str),
            Some("format/cell")
        );
        fs::remove_dir_all(root).expect("remove project root");
    }
}
