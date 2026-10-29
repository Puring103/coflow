//! Project/global plugin persistence.

use super::manifest::valid_plugin_id;
use super::manifest::{
    FrontendPluginBundle, FrontendPlugins, PluginScope, ProjectFrontendPlugins,
    ProjectPluginDefaults,
};
use crate::plugin_manifest::PluginManifest;
use cfd_editor_core::editor::EditorError;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

const PROJECT_PLUGIN_DIR: &str = "editor-setting";
const PROJECT_PLUGIN_FILE: &str = "plugins.json";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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
    // 一次打开读取完整快照；Windows 标准文件句柄允许 DELETE sharing，读取不创建项目文件。
    // 写者只以原子替换发布，因此已打开句柄始终指向替换前或替换后的完整配置。
    let contents = match std::fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ProjectPluginsFile {
                version: project_plugin_file_version(),
                ..ProjectPluginsFile::default()
            })
        }
        Err(error) => {
            return Err(EditorError::other(format!(
                "failed to read {}: {error}",
                path.display()
            )))
        }
    };
    let mut plugins: ProjectPluginsFile = serde_json::from_str(&contents).map_err(|error| {
        EditorError::other(format!("failed to parse {}: {error}", path.display()))
    })?;
    normalize_plugin_paths(project_root, &mut plugins)?;
    Ok(plugins)
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
    let mut plugins = plugins.clone();
    normalize_plugin_paths(project_root, &mut plugins)?;
    let contents = serde_json::to_string_pretty(&plugins).map_err(|error| {
        EditorError::other(format!("failed to encode project plugins: {error}"))
    })?;
    // 同目录暂存后一次替换；std::fs::rename 支持 Windows 上仍被读取的旧快照，读者无需创建锁。
    let publish = || -> std::io::Result<()> {
        let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
        temporary.write_all(contents.as_bytes())?;
        temporary.as_file().sync_all()?;
        let temporary = temporary.into_temp_path();
        std::fs::rename(&temporary, &path)
    };
    publish()
        .map_err(|error| EditorError::other(format!("failed to write {}: {error}", path.display())))
}

/// 独立锁文件覆盖整个读改写，原子替换配置后才释放，避免跨窗口或跨进程丢失更新。
fn mutate_project_plugins(
    project_root: &Path,
    mutate: impl FnOnce(&mut ProjectPluginsFile) -> Result<(), EditorError>,
) -> Result<(), EditorError> {
    let _lock = lock_project_plugins(project_root)?;
    let mut config = read_project_plugins(project_root)?;
    mutate(&mut config)?;
    write_project_plugins(project_root, &config)
}

fn lock_project_plugins(project_root: &Path) -> Result<std::fs::File, EditorError> {
    let path = project_plugins_path(project_root).with_extension("lock");
    let parent = path
        .parent()
        .ok_or_else(|| EditorError::other("project plugin lock has no parent"))?;
    std::fs::create_dir_all(parent).map_err(|error| {
        EditorError::other(format!("failed to create {}: {error}", parent.display()))
    })?;
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&path)
        .map_err(|error| {
            EditorError::other(format!("failed to open {}: {error}", path.display()))
        })?;
    FileExt::lock_exclusive(&lock).map_err(|error| {
        EditorError::other(format!("failed to lock {}: {error}", path.display()))
    })?;
    Ok(lock)
}

// 旧插件配置的绝对路径在读取时转换，下次保存统一写回相对路径。
fn normalize_plugin_paths(
    project_root: &Path,
    plugins: &mut ProjectPluginsFile,
) -> Result<(), EditorError> {
    for entry in &mut plugins.plugins {
        let path = Path::new(&entry.manifest);
        if path.is_absolute() {
            entry.manifest =
                coflow_runtime::path_to_slash(&relative_project_path(project_root, path)?);
        }
    }
    Ok(())
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
    mutate_project_plugins(project_root, |config| {
        config.version = project_plugin_file_version();
        config.plugins.retain(|entry| entry.id != bundle.id);
        config.plugins.push(ProjectPluginEntry {
            id: bundle.id.clone(),
            manifest: relative.to_string_lossy().replace('\\', "/"),
            enabled: true,
        });
        Ok(())
    })?;
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
    mutate_project_plugins(project_root, |config| {
        let before = config.plugins.len();
        config.plugins.retain(|entry| entry.id != id);
        if config.plugins.len() == before {
            return Err(EditorError::not_found(format!(
                "project plugin `{id}` not found"
            )));
        }
        Ok(())
    })
}

pub(crate) fn update_project_frontend_plugin_enabled(
    project_root: &Path,
    id: &str,
    enabled: bool,
) -> Result<(), EditorError> {
    mutate_project_plugins(project_root, |config| {
        let entry = config
            .plugins
            .iter_mut()
            .find(|entry| entry.id == id)
            .ok_or_else(|| EditorError::not_found(format!("project plugin `{id}` not found")))?;
        entry.enabled = enabled;
        Ok(())
    })
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
        install_project_frontend_plugin_bundle, mutate_project_plugins, project_frontend_plugins,
        project_plugins_path, read_project_plugins, remove_project_frontend_plugin,
        resolve_project_manifest, update_project_frontend_plugin_enabled, write_project_plugins,
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
    fn reading_project_plugins_never_creates_project_state_or_requires_a_writable_lock() {
        let root = temp_plugin_dir("read-only");
        assert!(read_project_plugins(&root)
            .expect("missing config")
            .plugins
            .is_empty());
        assert!(!root.exists());

        let path = project_plugins_path(&root);
        fs::create_dir_all(path.parent().expect("config parent")).expect("config directory");
        fs::write(
            &path,
            r#"{"version":1,"defaults":{"views":{"Foo":"sample/table"}}}"#,
        )
        .expect("existing config");
        let lock = path.with_extension("lock");
        let config = read_project_plugins(&root).expect("config without a lock");
        assert_eq!(
            config.defaults.views.get("Foo").map(String::as_str),
            Some("sample/table")
        );
        assert!(!lock.exists());

        fs::write(&lock, "").expect("existing lock");
        let original = fs::metadata(&lock).expect("lock metadata").permissions();
        let mut readonly = original.clone();
        readonly.set_readonly(true);
        fs::set_permissions(&lock, readonly).expect("read-only lock");
        let result = read_project_plugins(&root);
        fs::set_permissions(&lock, original).expect("restore lock permissions");
        assert!(result.is_ok(), "{result:?}");
        fs::remove_dir_all(root).expect("remove project");
    }

    #[test]
    fn publishing_project_plugins_preserves_an_open_reader_snapshot() {
        use std::io::Read;

        let root = temp_plugin_dir("open-reader");
        mutate_project_plugins(&root, |config| {
            config
                .defaults
                .views
                .insert("Foo".into(), "sample/old".into());
            Ok(())
        })
        .expect("original config");
        let path = project_plugins_path(&root);
        let original = fs::read_to_string(&path).expect("original contents");
        let mut reader = fs::File::open(&path).expect("retained old reader");
        mutate_project_plugins(&root, |config| {
            config
                .defaults
                .views
                .insert("Foo".into(), "sample/new".into());
            Ok(())
        })
        .expect("atomic replacement with a live reader");
        let mut retained = String::new();
        reader.read_to_string(&mut retained).expect("old snapshot");
        assert_eq!(retained, original);
        assert_eq!(
            read_project_plugins(&root)
                .expect("new snapshot")
                .defaults
                .views
                .get("Foo")
                .map(String::as_str),
            Some("sample/new")
        );
        drop(reader);
        fs::remove_dir_all(root).expect("remove project");
    }

    #[test]
    fn concurrent_project_plugin_updates_preserve_entries_and_complete_reads() {
        use std::sync::{
            atomic::{AtomicBool, Ordering},
            Arc, Barrier,
        };

        let root = temp_plugin_dir("concurrent");
        mutate_project_plugins(&root, |config| {
            config
                .defaults
                .views
                .insert("Foo".into(), "sample/table".into());
            Ok(())
        })
        .expect("initial config");
        for worker in 0..8 {
            for index in 0..8 {
                let id = format!("plugin-{worker}-{index}");
                let plugin = root.join(&id);
                fs::create_dir_all(&plugin).expect("plugin directory");
                fs::write(
                    plugin.join("plugin.json"),
                    serde_json::json!({
                        "id": id, "name": "Concurrent plugin", "entry": "plugin.js",
                    })
                    .to_string(),
                )
                .expect("plugin manifest");
                fs::write(plugin.join("plugin.js"), "export default () => {}")
                    .expect("plugin module");
            }
        }
        let start = Arc::new(Barrier::new(9));
        let finished = AtomicBool::new(false);
        std::thread::scope(|scope| {
            // 读取与写入分别打开锁文件，必须始终得到完整 JSON。
            let reader = scope.spawn(|| {
                while !finished.load(Ordering::Acquire) {
                    let config = read_project_plugins(&root).expect("atomic config read");
                    assert_eq!(
                        config.defaults.views.get("Foo").map(String::as_str),
                        Some("sample/table")
                    );
                    std::thread::yield_now();
                }
            });
            let mut writers = Vec::new();
            for worker in 0..8 {
                let root = &root;
                let start = Arc::clone(&start);
                writers.push(scope.spawn(move || {
                    start.wait();
                    for index in 0..8 {
                        let id = format!("plugin-{worker}-{index}");
                        install_project_frontend_plugin_bundle(
                            root,
                            &root.join(&id).join("plugin.json"),
                        )
                        .expect("concurrent install");
                        update_project_frontend_plugin_enabled(root, &id, false)
                            .expect("disable own plugin");
                    }
                }));
            }
            start.wait();
            let results: Vec<_> = writers
                .into_iter()
                .map(std::thread::ScopedJoinHandle::join)
                .collect();
            finished.store(true, Ordering::Release);
            reader.join().expect("reader");
            for result in results {
                result.expect("writer");
            }
        });
        let config = read_project_plugins(&root).expect("final config");
        assert_eq!(config.plugins.len(), 64);
        assert!(config.plugins.iter().all(|entry| !entry.enabled));
        let original = fs::read(project_plugins_path(&root)).expect("original bytes");
        assert!(remove_project_frontend_plugin(&root, "missing").is_err());
        assert_eq!(
            fs::read(project_plugins_path(&root)).expect("unchanged bytes"),
            original
        );
        fs::remove_dir_all(root).expect("remove project root");
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
        assert_eq!(state.errors, [] as [String; 0]);
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
    fn project_plugin_config_converts_old_absolute_paths_on_read() {
        let root = temp_plugin_dir("migrate-absolute");
        fs::create_dir_all(root.join("editor-setting")).expect("settings directory");
        fs::create_dir_all(root.join("external")).expect("plugin directory");
        let manifest = root.join("external/plugin.json");
        fs::write(&manifest, "{}").expect("manifest");
        let path = project_plugins_path(&root);
        let original = serde_json::json!({
            "version": 1,
            "plugins": [{"id": "example", "manifest": manifest}]
        })
        .to_string();
        fs::write(&path, &original).expect("old config");
        let config = read_project_plugins(&root).expect("read old config");
        assert_eq!(config.plugins[0].manifest, "external/plugin.json");
        assert_eq!(
            fs::read_to_string(&path).expect("original config"),
            original
        );
        write_project_plugins(&root, &config).expect("save config");
        let saved: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(path).expect("saved config")).expect("json");
        assert_eq!(saved["plugins"][0]["manifest"], "external/plugin.json");
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
