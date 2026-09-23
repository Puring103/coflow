//! project wire types.
use coflow_project::FlatDiagnostic;
use coflow_project::{FileTreeNode, RecordCoordinate};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[cfg(feature = "ts-export")]
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
pub struct ProjectBootstrap {
    pub session_id: u32,
    pub revision: u32,
    /// schema 重建时推进，普通数据 mutation 保持不变。
    pub schema_revision: u32,
    pub project_root: String,
    pub file_tree: Vec<FileTreeNode>,
    #[serde(default)]
    pub file_types: BTreeMap<String, Vec<FileTypeOption>>,
    pub dimensions: Vec<coflow_project::DimensionInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_source_file: Option<String>,
    pub diagnostics: Vec<FlatDiagnostic>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
pub struct FileTypeOption {
    pub name: String,
    pub display_name: String,
    pub record_count: usize,
    pub is_singleton: bool,
    /// 该实际类型在各维度下可编辑的字段，已包含继承字段。
    pub dimension_fields: BTreeMap<String, Vec<String>>,
}

/// 提供给编辑器插件的只读 Schema 投影。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
pub struct PluginSchemaType {
    pub name: String,
    pub fields: Vec<PluginSchemaField>,
    pub is_singleton: bool,
    pub record_count: usize,
}

/// 插件 Schema 投影中的字段信息。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
pub struct PluginSchemaField {
    pub name: String,
    pub type_label: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
#[serde(rename_all = "snake_case")]
pub enum ProjectSearchMode {
    Key,
    FullText,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
pub struct ProjectSearchHit {
    pub file_path: String,
    pub coordinate: RecordCoordinate,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
pub struct ProjectSearchResults {
    pub revision: u32,
    pub hits: Vec<ProjectSearchHit>,
    pub truncated: bool,
}
