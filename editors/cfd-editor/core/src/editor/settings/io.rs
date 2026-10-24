//! 编辑器共享/本机设置的原子读写与版本校验。

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use atomicwrites::{AllowOverwrite, AtomicFile};
use serde::Deserialize;
use serde::Serialize;

use super::model::{
    LocalSettingsFile, SettingsFile, LOCAL_SETTINGS_VERSION, SETTINGS_FILE, SETTINGS_VERSION,
};
use crate::editor::types::{EditorError, EditorProjectSettings};
use sha2::{Digest, Sha256};

fn settings_path(project_root: &Path) -> PathBuf {
    project_root.join("editor-setting").join(SETTINGS_FILE)
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, EditorError> {
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
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share"))
            })
    }
    .ok_or_else(|| EditorError::other("user data directory is unavailable"))?;
    let root = coflow_runtime::canonicalize_path(project_root)
        .map_err(|error| EditorError::other(format!("failed to resolve project root: {error}")))?;
    let identity = coflow_runtime::path_to_slash(&root);
    let digest = Sha256::digest(identity.as_bytes());
    let key = format!("{digest:x}");
    Ok(base
        .join("coflow")
        .join("cfd-editor")
        .join("projects")
        .join(key)
        .join(SETTINGS_FILE))
}

/// 共享设置只包含项目字段，个人展示状态从本机设置读取。
pub(crate) fn read_project_settings(
    project_root: &Path,
) -> Result<EditorProjectSettings, EditorError> {
    let path = settings_path(project_root);
    let mut settings = if path.exists() {
        let file: SettingsFile = read_json(&path)?;
        if file.version != SETTINGS_VERSION {
            return Err(EditorError::other(format!(
                "unsupported editor settings version {}",
                file.version
            )));
        }
        file.into_runtime(project_root)?
    } else {
        EditorProjectSettings::default()
    };
    let local_path = local_settings_path(project_root)?;
    if local_path.exists() {
        let local: LocalSettingsFile = read_json(&local_path)?;
        if local.version != LOCAL_SETTINGS_VERSION {
            return Err(EditorError::other(format!(
                "unsupported local editor settings version {}",
                local.version
            )));
        }
        local.apply(project_root, &mut settings)?;
    }
    Ok(settings)
}

pub(crate) fn write_project_settings(
    project_root: &Path,
    settings: &EditorProjectSettings,
) -> Result<(), EditorError> {
    // 只有共享字段变化才更新 Git 文件，个人状态写入本机目录。
    let path = settings_path(project_root);
    let next = SettingsFile::from_runtime(project_root, settings)?;
    let bytes = serde_json::to_vec_pretty(&next).map_err(|error| {
        EditorError::other(format!("failed to encode editor settings: {error}"))
    })?;
    let changed = !path.exists()
        || fs::read(&path).map_err(|error| {
            EditorError::other(format!("failed to read {}: {error}", path.display()))
        })? != bytes;
    if changed {
        write_local_settings(project_root, settings)?;
        write_json(&path, &next)?;
    }
    Ok(())
}

pub(crate) fn write_local_settings(
    project_root: &Path,
    settings: &EditorProjectSettings,
) -> Result<(), EditorError> {
    let path = local_settings_path(project_root)?;
    let next = LocalSettingsFile::from_runtime(project_root, settings)?;
    let bytes = serde_json::to_vec_pretty(&next).map_err(|error| {
        EditorError::other(format!("failed to encode editor settings: {error}"))
    })?;
    if path.exists()
        && fs::read(&path).map_err(|error| {
            EditorError::other(format!("failed to read {}: {error}", path.display()))
        })? == bytes
    {
        return Ok(());
    }
    write_json(&path, &next)
}
