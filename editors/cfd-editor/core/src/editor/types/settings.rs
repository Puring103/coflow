//! settings wire types.
use coflow_runtime::RecordCoordinate;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[cfg(feature = "ts-export")]
use ts_rs::TS;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
pub struct EditorProjectSettings {
    /// 按图视图身份保存节点坐标，独立于数据版本，重新打开项目时恢复。
    #[serde(default)]
    pub graph_positions: BTreeMap<String, BTreeMap<String, [f64; 2]>>,
    /// 按图视图身份保存缩略/完整模式，独立于数据版本；缺省视为缩略模式。
    #[serde(default)]
    pub graph_compact_modes: BTreeMap<String, bool>,
    /// 每个记录类型唯一的缩略名字符串字段，跨文件生效。
    #[serde(default)]
    pub short_name_fields: BTreeMap<String, String>,
    /// 按文件和类型保存视图标签顺序，包含内置视图和自定义视图的 ID。
    #[serde(default)]
    pub view_order: BTreeMap<String, BTreeMap<String, Vec<String>>>,
    /// Custom views keyed by (filePath, actualType). Default record/table
    /// views are implicit and never stored here.
    #[serde(default)]
    pub views: BTreeMap<String, BTreeMap<String, Vec<ViewConfig>>>,
    /// Column widths for the implicit default table view, keyed by
    /// (filePath, actualType, columnName). Custom table views carry their
    /// own widths inside their [`ViewConfig`].
    #[serde(default)]
    pub default_table_column_widths: BTreeMap<String, BTreeMap<String, BTreeMap<String, f64>>>,
    #[serde(default)]
    pub record_groups: BTreeMap<String, BTreeMap<String, Vec<EditorRecordGroup>>>,
    #[serde(default)]
    pub workspace: EditorWorkspaceState,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
pub struct EditorWorkspaceState {
    #[serde(default)]
    pub tabs: Vec<EditorWorkspaceTab>,
    #[serde(default)]
    pub active_tab_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
pub struct EditorWorkspaceTab {
    pub file_path: String,
    pub type_name: String,
    pub view_id: String,
    pub view_kind: WorkspaceViewKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coordinate: Option<RecordCoordinate>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceViewKind {
    Record,
    Table,
    Graph,
    Source,
}

/// Kind of a custom view. Record view is implicit and cannot be created,
/// so it is not part of this enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
#[serde(rename_all = "snake_case")]
pub enum ViewKind {
    Table,
    Graph,
}

/// A user-created custom view over a (filePath, actualType).
///
/// Table views use `columns` (ordered) + `column_widths`; graph views use
/// `relations` + `fields`. `group_filter` is common to both (see design
/// doc §4). Unused fields for a given `kind` are cleared during sanitize.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
pub struct ViewConfig {
    pub id: String,
    pub name: String,
    pub kind: ViewKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_filter: Option<String>,
    #[serde(default)]
    pub columns: Vec<String>,
    #[serde(default)]
    pub column_widths: BTreeMap<String, f64>,
    #[serde(default)]
    pub relations: Vec<String>,
    #[serde(default)]
    pub fields: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
pub struct EditorRecordGroup {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub color: Option<String>,
    pub records: Vec<RecordCoordinate>,
}
