//! 编辑器设置磁盘模型：`editor-setting/editor.json` 的版本化结构与常量。
//!
//! 常量集中在此，IO 与清洗逻辑分别位于 `io` / `sanitize`。

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::editor::types::{
    EditorError, EditorProjectSettings, EditorRecordGroup, EditorWorkspaceState, ViewConfig,
};

pub(crate) const SETTINGS_FILE: &str = "editor.json";
pub(crate) const SETTINGS_VERSION: u8 = 1;
pub(crate) const LOCAL_SETTINGS_VERSION: u8 = 1;
pub(crate) const MIN_COLUMN_WIDTH: f64 = 48.0;
pub(crate) const MAX_VIEW_NAME_LEN: usize = 80;
pub(crate) const MAX_FIELD_LEN: usize = 160;
/// Reserved id prefix for implicit default views. User views cannot use it.
pub(crate) const RESERVED_VIEW_ID_PREFIX: &str = "__";
pub(crate) const RECORD_GROUP_COLORS: &[&str] = &[
    "red", "orange", "yellow", "green", "cyan", "blue", "purple", "gray",
];

/// 项目共享设置仅保存团队需要同步的视图结构和图布局。
#[derive(Debug, Default, Serialize, Deserialize)]
pub(crate) struct SettingsFile {
    pub(crate) version: u8,
    #[serde(default)]
    pub(crate) graph_positions: BTreeMap<String, BTreeMap<String, [f64; 2]>>,
    #[serde(default)]
    pub(crate) short_name_fields: BTreeMap<String, String>,
    #[serde(default)]
    pub(crate) views: BTreeMap<String, BTreeMap<String, Vec<SharedViewConfig>>>,
    #[serde(default)]
    pub(crate) record_groups: BTreeMap<String, BTreeMap<String, Vec<EditorRecordGroup>>>,
}

/// 共享视图不序列化列宽；未知 JSON 字段由 serde 默认忽略。
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct SharedViewConfig {
    id: String,
    name: String,
    kind: crate::editor::types::ViewKind,
    #[serde(default)] group_filter: Option<String>,
    #[serde(default)] columns: Vec<String>,
    #[serde(default)] relations: Vec<String>,
    #[serde(default)] fields: Vec<String>,
}

impl From<ViewConfig> for SharedViewConfig {
    fn from(view: ViewConfig) -> Self {
        Self { id: view.id, name: view.name, kind: view.kind, group_filter: view.group_filter,
            columns: view.columns, relations: view.relations, fields: view.fields }
    }
}

impl From<SharedViewConfig> for ViewConfig {
    fn from(view: SharedViewConfig) -> Self {
        Self { id: view.id, name: view.name, kind: view.kind, group_filter: view.group_filter,
            columns: view.columns, column_widths: BTreeMap::new(), relations: view.relations, fields: view.fields }
    }
}

/// 本机文件按项目分别存储，不写回项目目录。
#[derive(Debug, Default, Serialize, Deserialize)]
pub(crate) struct LocalSettingsFile {
    pub(crate) version: u8,
    #[serde(default)]
    pub(crate) graph_compact_modes: BTreeMap<String, bool>,
    #[serde(default)]
    pub(crate) view_order: BTreeMap<String, BTreeMap<String, Vec<String>>>,
    #[serde(default)]
    pub(crate) default_table_column_widths:
        BTreeMap<String, BTreeMap<String, BTreeMap<String, f64>>>,
    #[serde(default)]
    pub(crate) view_column_widths:
        BTreeMap<String, BTreeMap<String, BTreeMap<String, BTreeMap<String, f64>>>>,
    #[serde(default)]
    pub(crate) workspace: EditorWorkspaceState,
}

impl SettingsFile {
    pub(crate) fn from_runtime(project_root: &Path, settings: &EditorProjectSettings) -> Result<Self, EditorError> {
        // 列宽是个人偏好，不得混入 Git 跟踪的视图定义。
        let views = settings.views.iter().map(|(path, by_type)| {
            (path.clone(), by_type.iter().map(|(ty, entries)| {
                (ty.clone(), entries.iter().cloned().map(SharedViewConfig::from).collect())
            }).collect())
        }).collect();
        let mut file = Self {
            version: SETTINGS_VERSION,
            graph_positions: settings.graph_positions.clone(),
            short_name_fields: settings.short_name_fields.clone(),
            views,
            record_groups: settings.record_groups.clone(),
        };
        file.views = map_outer_keys(file.views, &mut |path| stored_file_path(project_root, path))?;
        file.record_groups = map_outer_keys(file.record_groups, &mut |path| stored_file_path(project_root, path))?;
        file.graph_positions = map_graph_keys(file.graph_positions, &mut |path| stored_file_path(project_root, path))?;
        Ok(file)
    }

    pub(crate) fn into_runtime(mut self, project_root: &Path) -> Result<EditorProjectSettings, EditorError> {
        self.views = map_outer_keys(self.views, &mut |path| Ok(runtime_file_path(project_root, path)))?;
        self.record_groups = map_outer_keys(self.record_groups, &mut |path| Ok(runtime_file_path(project_root, path)))?;
        self.graph_positions = map_graph_keys(self.graph_positions, &mut |path| Ok(runtime_file_path(project_root, path)))?;
        Ok(EditorProjectSettings {
            graph_positions: self.graph_positions,
            short_name_fields: self.short_name_fields,
            views: self.views.into_iter().map(|(path, by_type)| {
                (path, by_type.into_iter().map(|(ty, entries)| {
                    (ty, entries.into_iter().map(ViewConfig::from).collect())
                }).collect())
            }).collect(),
            record_groups: self.record_groups,
            ..EditorProjectSettings::default()
        })
    }
}

impl LocalSettingsFile {
    pub(crate) fn from_runtime(project_root: &Path, settings: &EditorProjectSettings) -> Result<Self, EditorError> {
        let mut file = Self {
            version: LOCAL_SETTINGS_VERSION,
            graph_compact_modes: settings.graph_compact_modes.clone(),
            view_order: settings.view_order.clone(),
            default_table_column_widths: settings.default_table_column_widths.clone(),
            view_column_widths: settings.views.iter().filter_map(|(path, by_type)| {
                let widths = by_type.iter().filter_map(|(ty, views)| {
                    let widths = views.iter().filter(|view| !view.column_widths.is_empty())
                        .map(|view| (view.id.clone(), view.column_widths.clone())).collect::<BTreeMap<_, _>>();
                    (!widths.is_empty()).then_some((ty.clone(), widths))
                }).collect::<BTreeMap<_, _>>();
                (!widths.is_empty()).then_some((path.clone(), widths))
            }).collect(),
            workspace: settings.workspace.clone(),
        };
        file.map_file_paths(|path| stored_file_path(project_root, path))?;
        Ok(file)
    }

    pub(crate) fn apply(mut self, project_root: &Path, settings: &mut EditorProjectSettings) -> Result<(), EditorError> {
        self.map_file_paths(|path| Ok(runtime_file_path(project_root, path)))?;
        settings.graph_compact_modes = self.graph_compact_modes;
        settings.view_order = self.view_order;
        settings.default_table_column_widths = self.default_table_column_widths;
        settings.workspace = self.workspace;
        for (path, by_type) in self.view_column_widths {
            if let Some(views_by_type) = settings.views.get_mut(&path) {
                for (ty, widths_by_id) in by_type {
                    if let Some(views) = views_by_type.get_mut(&ty) {
                        for view in views {
                            if let Some(widths) = widths_by_id.get(&view.id) {
                                view.column_widths = super::sanitize::sanitized_column_widths(widths.clone());
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn map_file_paths(&mut self, mut map: impl FnMut(&str) -> Result<String, EditorError>) -> Result<(), EditorError> {
        self.view_order = map_outer_keys(std::mem::take(&mut self.view_order), &mut map)?;
        self.default_table_column_widths = map_outer_keys(std::mem::take(&mut self.default_table_column_widths), &mut map)?;
        self.view_column_widths = map_outer_keys(std::mem::take(&mut self.view_column_widths), &mut map)?;
        self.graph_compact_modes = map_graph_keys(std::mem::take(&mut self.graph_compact_modes), &mut map)?;
        // 维度标签的文件路径是虚拟入口，迁移时只转换其业务文件并同步当前标签 ID。
        for tab in &mut self.workspace.tabs {
            if let Some(target) = &mut tab.dimension_target {
                let previous = serde_json::to_string(&[
                    &target.dimension, &target.owner_file, &target.type_name,
                    target.field.as_deref().unwrap_or(""),
                ]).map_err(|error| EditorError::other(error.to_string()))?;
                target.owner_file = map(&target.owner_file)?;
                if self.workspace.active_tab_id.as_deref() == Some(&previous) {
                    self.workspace.active_tab_id = Some(serde_json::to_string(&[
                        &target.dimension, &target.owner_file, &target.type_name,
                        target.field.as_deref().unwrap_or(""),
                    ]).map_err(|error| EditorError::other(error.to_string()))?);
                }
            } else {
                tab.file_path = map(&tab.file_path)?;
            }
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
    coflow_project::project_path(project_root, Path::new(stored))
}

fn stored_file_path(project_root: &Path, runtime: &str) -> Result<String, EditorError> {
    let path = Path::new(runtime);
    if !path.is_absolute() {
        return Ok(coflow_project::path_to_slash(&normalize_relative(path)));
    }
    let root = coflow_project::normalize_path(project_root);
    let path = coflow_project::normalize_path(path);
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
    Ok(coflow_project::path_to_slash(&relative))
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
