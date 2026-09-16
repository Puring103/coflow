//! 编辑器设置磁盘模型：`editor-setting/editor.json` 的版本化结构与常量。
//!
//! 常量集中在此，IO 与清洗逻辑分别位于 `io` / `sanitize`。

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::editor::types::{
    EditorError, EditorProjectSettings, EditorRecordGroup, EditorWorkspaceState,
};

pub(crate) const SETTINGS_FILE: &str = "editor.json";
pub(crate) const SETTINGS_VERSION: u8 = 1;
pub(crate) const MIN_COLUMN_WIDTH: f64 = 48.0;
pub(crate) const MAX_VIEW_NAME_LEN: usize = 80;
pub(crate) const MAX_FIELD_LEN: usize = 160;
/// Reserved id prefix for implicit default views. User views cannot use it.
pub(crate) const RESERVED_VIEW_ID_PREFIX: &str = "__";
pub(crate) const RECORD_GROUP_COLORS: &[&str] = &[
    "red", "orange", "yellow", "green", "cyan", "blue", "purple", "gray",
];

/// Versioned on-disk shape of `editor-setting/editor.json`.
#[derive(Debug, Default, Serialize, Deserialize)]
pub(crate) struct SettingsFile {
    pub(crate) version: u8,
    #[serde(default)]
    pub(crate) graph_positions: BTreeMap<String, BTreeMap<String, [f64; 2]>>,
    #[serde(default)]
    pub(crate) graph_compact_modes: BTreeMap<String, bool>,
    #[serde(default)]
    pub(crate) short_name_fields: BTreeMap<String, String>,
    #[serde(default)]
    pub(crate) view_order: BTreeMap<String, BTreeMap<String, Vec<String>>>,
    #[serde(default)]
    pub(crate) views: BTreeMap<String, BTreeMap<String, Vec<crate::editor::types::ViewConfig>>>,
    #[serde(default)]
    pub(crate) default_table_column_widths:
        BTreeMap<String, BTreeMap<String, BTreeMap<String, f64>>>,
    #[serde(default)]
    pub(crate) record_groups: BTreeMap<String, BTreeMap<String, Vec<EditorRecordGroup>>>,
    #[serde(default)]
    pub(crate) workspace: EditorWorkspaceState,
}

impl SettingsFile {
    pub(crate) fn from_runtime(
        project_root: &Path,
        settings: &EditorProjectSettings,
    ) -> Result<Self, EditorError> {
        let mut file = Self {
            version: SETTINGS_VERSION,
            graph_positions: settings.graph_positions.clone(),
            graph_compact_modes: settings.graph_compact_modes.clone(),
            short_name_fields: settings.short_name_fields.clone(),
            view_order: settings.view_order.clone(),
            views: settings.views.clone(),
            default_table_column_widths: settings.default_table_column_widths.clone(),
            record_groups: settings.record_groups.clone(),
            workspace: settings.workspace.clone(),
        };
        file.map_file_paths(|path| stored_file_path(project_root, path))?;
        Ok(file)
    }

    pub(crate) fn into_runtime(
        mut self,
        project_root: &Path,
    ) -> Result<EditorProjectSettings, EditorError> {
        self.map_file_paths(|path| Ok(runtime_file_path(project_root, path)))?;
        Ok(EditorProjectSettings {
            graph_positions: self.graph_positions,
            graph_compact_modes: self.graph_compact_modes,
            short_name_fields: self.short_name_fields,
            view_order: self.view_order,
            views: self.views,
            default_table_column_widths: self.default_table_column_widths,
            record_groups: self.record_groups,
            workspace: self.workspace,
        })
    }

    fn map_file_paths(
        &mut self,
        mut map: impl FnMut(&str) -> Result<String, EditorError>,
    ) -> Result<(), EditorError> {
        self.view_order = map_outer_keys(std::mem::take(&mut self.view_order), &mut map)?;
        self.views = map_outer_keys(std::mem::take(&mut self.views), &mut map)?;
        self.default_table_column_widths = map_outer_keys(
            std::mem::take(&mut self.default_table_column_widths),
            &mut map,
        )?;
        self.record_groups = map_outer_keys(std::mem::take(&mut self.record_groups), &mut map)?;
        self.graph_positions = map_graph_keys(std::mem::take(&mut self.graph_positions), &mut map)?;
        self.graph_compact_modes =
            map_graph_keys(std::mem::take(&mut self.graph_compact_modes), &mut map)?;

        for tab in &mut self.workspace.tabs {
            tab.file_path = map(&tab.file_path)?;
        }
        if let Some(active) = &mut self.workspace.active_tab_id {
            if let Some((path, suffix)) = active.split_once('\u{1f}') {
                *active = format!("{}\u{1f}{suffix}", map(path)?);
            }
        }
        Ok(())
    }
}

fn map_outer_keys<T>(
    values: BTreeMap<String, T>,
    map: &mut impl FnMut(&str) -> Result<String, EditorError>,
) -> Result<BTreeMap<String, T>, EditorError> {
    values
        .into_iter()
        .map(|(path, value)| Ok((map(&path)?, value)))
        .collect()
}

fn map_graph_keys<T>(
    values: BTreeMap<String, T>,
    map: &mut impl FnMut(&str) -> Result<String, EditorError>,
) -> Result<BTreeMap<String, T>, EditorError> {
    values
        .into_iter()
        .map(|(key, value)| {
            let mut parts = serde_json::from_str::<[String; 3]>(&key).map_err(|error| {
                EditorError::other(format!(
                    "invalid graph view key in editor settings: {error}"
                ))
            })?;
            parts[0] = map(&parts[0])?;
            let key = serde_json::to_string(&parts).map_err(|error| {
                EditorError::other(format!("failed to encode graph view key: {error}"))
            })?;
            Ok((key, value))
        })
        .collect()
}

fn runtime_file_path(project_root: &Path, stored: &str) -> String {
    coflow_runtime::project_path(project_root, Path::new(stored))
}

fn stored_file_path(project_root: &Path, runtime: &str) -> Result<String, EditorError> {
    let path = Path::new(runtime);
    if !path.is_absolute() {
        return Ok(coflow_runtime::path_to_slash(&normalize_relative(path)));
    }
    let root = coflow_runtime::normalize_path(project_root);
    let path = coflow_runtime::normalize_path(path);
    let root_components = root.components().collect::<Vec<_>>();
    let path_components = path.components().collect::<Vec<_>>();
    let common = root_components
        .iter()
        .zip(&path_components)
        .take_while(|(left, right)| left == right)
        .count();
    if common == 0 {
        return Err(EditorError::other(format!(
            "editor settings path `{}` cannot be made relative to project root `{}`",
            path.display(),
            root.display()
        )));
    }
    let mut relative = PathBuf::new();
    for component in &root_components[common..] {
        if matches!(component, Component::Normal(_)) {
            relative.push("..");
        }
    }
    for component in &path_components[common..] {
        relative.push(component.as_os_str());
    }
    Ok(coflow_runtime::path_to_slash(&relative))
}

fn normalize_relative(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir
                if matches!(
                    normalized.components().next_back(),
                    Some(Component::Normal(_))
                ) =>
            {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}
