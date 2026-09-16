//! Project/global plugin persistence.

use super::manifest::valid_plugin_id;
use super::manifest::{
    FrontendPluginBundle, FrontendPlugins, PluginScope, ProjectFrontendPlugins,
    ProjectPluginDefaults,
};
use crate::plugin_manifest::PluginManifest;
use cfd_editor_core::editor::EditorError;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

const PROJECT_PLUGIN_DIR: &str = "editor-setting";
const PROJECT_PLUGIN_FILE: &str = "plugins.json";

#[derive(Debug, Default, Serialize, Deserialize)]
pub(crate) struct ProjectPluginsFile {
    #[serde(default = "project_plugin_file_version")]
    version: u32,
    #[serde(default)]
    plugins: Vec<ProjectPluginEntry>,
    #[serde(default)]
    defaults: ProjectPluginDefaults,
}

const fn project_plugin_file_version() -> u32 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ProjectPluginEntry {
    id: String,
    manifest: String,
    #[serde(default = "default_plugin_enabled")]
    enabled: bool,
}

const fn default_plugin_enabled() -> bool {
    true
}

pub(crate) fn project_plugins_path(project_root: &Path) -> PathBuf {
    project_root
        .join(PROJECT_PLUGIN_DIR)
        .join(PROJECT_PLUGIN_FILE)
}

pub(crate) fn read_project_plugins(project_root: &Path) -> Result<ProjectPluginsFile, EditorError> {
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

pub(crate) fn write_project_plugins(
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

pub(crate) fn relative_project_path(
    project_root: &Path,
    target: &Path,
) -> Result<PathBuf, EditorError> {
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

pub(crate) fn resolve_project_manifest(
    project_root: &Path,
    manifest: &str,
) -> Result<PathBuf, EditorError> {
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

pub(crate) fn project_bundle(
    project_root: &Path,
    entry: &ProjectPluginEntry,
) -> Result<FrontendPluginBundle, EditorError> {
    let manifest = resolve_project_manifest(project_root, &entry.manifest)?;
    let mut bundle = super::manifest::load_frontend_plugin_bundle(&manifest)?;
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

pub(crate) fn install_project_frontend_plugin_bundle(
    project_root: &Path,
    manifest: &Path,
) -> Result<FrontendPluginBundle, EditorError> {
    let mut bundle = super::manifest::load_frontend_plugin_bundle(manifest)?;
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

pub(crate) fn project_frontend_plugins(
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

pub(crate) fn remove_project_frontend_plugin(
    project_root: &Path,
    id: &str,
) -> Result<(), EditorError> {
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

pub(crate) fn update_project_frontend_plugin_enabled(
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

pub(crate) fn plugin_data_dir(app: &AppHandle) -> Result<PathBuf, EditorError> {
    app.path()
        .app_data_dir()
        .map(|path| path.join("plugins"))
        .map_err(|error| {
            EditorError::other(format!("failed to resolve plugin data directory: {error}"))
        })
}

pub(crate) fn install_frontend_plugin_bundle(
    manifest_path: &Path,
    app: &AppHandle,
) -> Result<FrontendPluginBundle, EditorError> {
    let bundle = super::manifest::load_frontend_plugin_bundle(manifest_path)?;
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
    super::manifest::load_frontend_plugin_bundle(&plugin_dir.join("plugin.json"))
}

pub(crate) fn list_frontend_plugin_bundles(
    app: &AppHandle,
) -> Result<FrontendPlugins, EditorError> {
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
        match super::manifest::load_frontend_plugin_bundle(&manifest) {
            Ok(bundle) => plugins.push(bundle),
            Err(error) => errors.push(format!("{}: {}", manifest.display(), error.message)),
        }
    }
    plugins.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(FrontendPlugins { plugins, errors })
}

pub(crate) fn uninstall_frontend_plugin_bundle(
    id: &str,
    app: &AppHandle,
) -> Result<(), EditorError> {
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

    use super::super::manifest::{load_frontend_plugin_bundle, PluginScope};
    use super::{
        install_project_frontend_plugin_bundle, project_frontend_plugins, read_project_plugins,
        resolve_project_manifest,
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
        assert!(matches!(bundle.scope, PluginScope::Project));
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
