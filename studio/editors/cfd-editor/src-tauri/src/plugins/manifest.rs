//! Tauri 前端插件的命令、持久化与路径安全边界。

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use crate::plugin_manifest::PluginManifest;
use cfd_editor_core::editor::EditorError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
pub struct FrontendPluginBundle {
    pub(crate) manifest_path: String,
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) version: String,
    pub(crate) source: String,
    pub(crate) scope: PluginScope,
    pub(crate) enabled: bool,
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
    pub(crate) plugins: Vec<FrontendPluginBundle>,
    pub(crate) errors: Vec<String>,
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

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
pub struct ProjectPluginDefaults {
    #[serde(default)]
    pub(crate) views: BTreeMap<String, String>,
    #[serde(default)]
    pub(crate) presentations: BTreeMap<String, BTreeMap<String, String>>,
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
    pub(crate) plugins: Vec<FrontendPluginBundle>,
    pub(crate) defaults: ProjectPluginDefaults,
    pub(crate) errors: Vec<String>,
}

pub(crate) fn load_frontend_plugin_bundle(
    manifest_path: &Path,
) -> Result<FrontendPluginBundle, EditorError> {
    if manifest_path
        .extension()
        .is_none_or(|extension| extension != "json")
    {
        return Err(EditorError::other("plugin manifest must be a .json file"));
    }
    let manifest_path = coflow_project::canonicalize_path(manifest_path)
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
    let entry_path = coflow_project::canonicalize_path(plugin_dir.join(entry))
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

pub(crate) fn valid_plugin_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
}
