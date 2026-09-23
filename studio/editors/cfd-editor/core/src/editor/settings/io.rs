//! 编辑器共享/本机设置的原子读写、版本校验与旧文件迁移。

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use atomicwrites::{AllowOverwrite, AtomicFile};
use serde::Deserialize;
use serde::Serialize;

use super::model::{LocalSettingsFile, SettingsFile, LOCAL_SETTINGS_VERSION, SETTINGS_FILE, SETTINGS_VERSION};
use sha2::{Digest, Sha256};
use crate::editor::types::{EditorError, EditorProjectSettings};

fn settings_path(project_root: &Path) -> PathBuf {
    project_root.join("editor-setting").join(SETTINGS_FILE)
}

fn read_json<T: Default + for<'de> Deserialize<'de>>(path: &Path) -> Result<T, EditorError> {
    if !path.exists() {
        return Ok(T::default());
    }
    let bytes = fs::read(path).map_err(|error| {
        EditorError::other(format!("failed to read {}: {error}", path.display()))
    })?;
    serde_json::from_slice(&bytes)
        .map_err(|error| EditorError::other(format!("failed to parse {}: {error}", path.display())))
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), EditorError> {
    let parent = path
        .parent()
        .ok_or_else(|| EditorError::other("editor settings path has no parent"))?;
    fs::create_dir_all(parent).map_err(|error| {
        EditorError::other(format!("failed to create {}: {error}", parent.display()))
    })?;
    let bytes = serde_json::to_vec_pretty(value).map_err(|error| {
        EditorError::other(format!("failed to encode editor settings: {error}"))
    })?;
    AtomicFile::new(path, AllowOverwrite)
        .write(|file| file.write_all(&bytes))
        .map_err(|error| EditorError::other(format!("failed to write {}: {error}", path.display())))
}

/// 本机设置放在用户数据目录，以规范化项目路径的摘要隔离各项目。
pub(super) fn local_settings_path(project_root: &Path) -> Result<PathBuf, EditorError> {
    let base = if cfg!(target_os = "windows") {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|home| PathBuf::from(home).join("Library/Application Support"))
    } else {
        std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).or_else(|| {
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share"))
        })
    }.ok_or_else(|| EditorError::other("user data directory is unavailable"))?;
    let root = coflow_project::canonicalize_path(project_root)
        .map_err(|error| EditorError::other(format!("failed to resolve project root: {error}")))?;
    let identity = coflow_project::path_to_slash(&root);
    let digest = Sha256::digest(identity.as_bytes());
    let key = digest.iter().map(|byte| format!("{byte:02x}")).collect::<String>();
    Ok(base.join("coflow").join("cfd-editor").join("projects").join(key).join(SETTINGS_FILE))
}

/// 读取共享设置及本机覆盖；旧格式中的个人字段只在首次迁移时读取。
pub(crate) fn read_project_settings(project_root: &Path) -> Result<EditorProjectSettings, EditorError> {
    let path = settings_path(project_root);
    let mut settings = if path.exists() {
        let file: SettingsFile = read_json(&path)?;
        if file.version != SETTINGS_VERSION {
            return Err(EditorError::other(format!("unsupported editor settings version {}", file.version)));
        }
        file.into_runtime(project_root)?
    } else {
        EditorProjectSettings::default()
    };
    let local_path = local_settings_path(project_root)?;
    if local_path.exists() {
        let local: LocalSettingsFile = read_json(&local_path)?;
        if local.version != LOCAL_SETTINGS_VERSION {
            return Err(EditorError::other(format!("unsupported local editor settings version {}", local.version)));
        }
        local.apply(project_root, &mut settings)?;
    } else if path.exists() && has_legacy_personal_settings(&path)? {
        // 旧版共享文件的个人字段只迁移一次，先落本机，避免后续覆盖个人改动。
        let legacy: LegacyPersonalSettings = read_json(&path)?;
        legacy.apply(project_root, &mut settings)?;
        write_json(&local_path, &LocalSettingsFile::from_runtime(project_root, &settings)?)?;
    }
    Ok(settings)
}

fn has_legacy_personal_settings(path: &Path) -> Result<bool, EditorError> {
    let bytes = fs::read(path).map_err(|error| {
        EditorError::other(format!("failed to read {}: {error}", path.display()))
    })?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|error| {
        EditorError::other(format!("failed to parse {}: {error}", path.display()))
    })?;
    Ok(["graph_compact_modes", "view_order", "default_table_column_widths", "workspace"]
        .iter()
        .any(|key| value.get(key).is_some())
        || value.get("views").and_then(serde_json::Value::as_object).is_some_and(|files| {
            files.values().any(|types| types.as_object().is_some_and(|types| {
                types.values().any(|views| views.as_array().is_some_and(|entries| {
                    entries.iter().any(|view| view.get("column_widths").is_some())
                }))
            }))
        }))
}

#[derive(Default, Deserialize)]
struct LegacyPersonalSettings {
    #[serde(default)]
    graph_compact_modes: std::collections::BTreeMap<String, bool>,
    #[serde(default)]
    view_order: std::collections::BTreeMap<String, std::collections::BTreeMap<String, Vec<String>>>,
    #[serde(default)]
    default_table_column_widths: std::collections::BTreeMap<String, std::collections::BTreeMap<String, std::collections::BTreeMap<String, f64>>>,
    #[serde(default)]
    workspace: crate::editor::types::EditorWorkspaceState,
    #[serde(default)]
    views: std::collections::BTreeMap<String, std::collections::BTreeMap<String, Vec<LegacyViewWidths>>>,
}

#[derive(Deserialize)]
struct LegacyViewWidths {
    id: String,
    #[serde(default)]
    column_widths: std::collections::BTreeMap<String, f64>,
}

impl LegacyPersonalSettings {
    fn apply(self, root: &Path, settings: &mut EditorProjectSettings) -> Result<(), EditorError> {
        // 复用本机结构的路径转换，确保外部文件和旧视图列宽一并迁移。
        let widths = self.views.into_iter().map(|(path, by_type)| {
            (path, by_type.into_iter().map(|(ty, views)| {
                (ty, views.into_iter().map(|view| (view.id, view.column_widths)).collect())
            }).collect())
        }).collect();
        LocalSettingsFile {
            version: LOCAL_SETTINGS_VERSION,
            graph_compact_modes: self.graph_compact_modes,
            view_order: self.view_order,
            default_table_column_widths: self.default_table_column_widths,
            view_column_widths: widths,
            workspace: self.workspace,
        }.apply(root, settings)
    }
}

pub(crate) fn write_project_settings(project_root: &Path, settings: &EditorProjectSettings) -> Result<(), EditorError> {
    // 只有共享字段变化才更新 Git 文件；旧版个人字段在本机迁移成功后清除。
    let path = settings_path(project_root);
    let next = SettingsFile::from_runtime(project_root, settings)?;
    let bytes = serde_json::to_vec_pretty(&next).map_err(|error| {
        EditorError::other(format!("failed to encode editor settings: {error}"))
    })?;
    let changed = !path.exists() || fs::read(&path).map_err(|error| {
        EditorError::other(format!("failed to read {}: {error}", path.display()))
    })? != bytes;
    if changed {
        write_local_settings(project_root, settings)?;
        write_json(&path, &next)?;
    }
    Ok(())
}

pub(crate) fn write_local_settings(project_root: &Path, settings: &EditorProjectSettings) -> Result<(), EditorError> {
    let path = local_settings_path(project_root)?;
    let next = LocalSettingsFile::from_runtime(project_root, settings)?;
    let bytes = serde_json::to_vec_pretty(&next).map_err(|error| {
        EditorError::other(format!("failed to encode editor settings: {error}"))
    })?;
    if path.exists() && fs::read(&path).map_err(|error| {
        EditorError::other(format!("failed to read {}: {error}", path.display()))
    })? == bytes {
        return Ok(());
    }
    write_json(&path, &next)
}
