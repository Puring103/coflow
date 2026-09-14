//! 编辑器设置磁盘模型：`editor-setting/editor.json` 的版本化结构与常量。
//!
//! 常量集中在此，IO 与清洗逻辑分别位于 `io` / `sanitize`。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::editor::types::{EditorProjectSettings, EditorRecordGroup, EditorWorkspaceState};

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

impl From<&EditorProjectSettings> for SettingsFile {
    fn from(settings: &EditorProjectSettings) -> Self {
        Self {
            version: SETTINGS_VERSION,
            graph_positions: settings.graph_positions.clone(),
            graph_compact_modes: settings.graph_compact_modes.clone(),
            short_name_fields: settings.short_name_fields.clone(),
            view_order: settings.view_order.clone(),
            views: settings.views.clone(),
            default_table_column_widths: settings.default_table_column_widths.clone(),
            record_groups: settings.record_groups.clone(),
            workspace: settings.workspace.clone(),
        }
    }
}

impl From<SettingsFile> for EditorProjectSettings {
    fn from(file: SettingsFile) -> Self {
        Self {
            graph_positions: file.graph_positions,
            graph_compact_modes: file.graph_compact_modes,
            short_name_fields: file.short_name_fields,
            view_order: file.view_order,
            views: file.views,
            default_table_column_widths: file.default_table_column_widths,
            record_groups: file.record_groups,
            workspace: file.workspace,
        }
    }
}
