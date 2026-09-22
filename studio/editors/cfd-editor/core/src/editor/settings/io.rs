//! 编辑器设置磁盘 IO：`editor.json` 的原子读写。
//!
//! 版本校验在此完成；结构转换由 `model` 负责。

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use atomicwrites::{AllowOverwrite, AtomicFile};
use serde::Deserialize;
use serde::Serialize;

use super::model::{SettingsFile, SETTINGS_FILE, SETTINGS_VERSION};
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

/// Read the single versioned editor settings file.
pub(crate) fn read_project_settings(
    project_root: &Path,
) -> Result<EditorProjectSettings, EditorError> {
    let path = settings_path(project_root);
    if !path.exists() {
        return Ok(EditorProjectSettings::default());
    }
    let settings: SettingsFile = read_json(&path)?;
    if settings.version != SETTINGS_VERSION {
        return Err(EditorError::other(format!(
            "unsupported editor settings version {}",
            settings.version
        )));
    }
    settings.into_runtime(project_root)
}

pub(crate) fn write_project_settings(
    project_root: &Path,
    settings: &EditorProjectSettings,
) -> Result<(), EditorError> {
    write_json(
        &settings_path(project_root),
        &SettingsFile::from_runtime(project_root, settings)?,
    )
}
