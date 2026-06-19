use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ProjectSnapshot {
    pub session_id: u32,
    pub file_tree: Vec<FileTreeNode>,
    pub diagnostics: Vec<DiagnosticItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct FileTreeNode {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub in_sources: bool,
    pub children: Vec<FileTreeNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DiagnosticItem {
    pub severity: String,
    pub code: String,
    pub stage: String,
    pub message: String,
    pub file_path: Option<String>,
    pub record_key: Option<String>,
    pub field_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct FileRecords {
    pub file_path: String,
    pub type_names: Vec<String>,
    pub records: Vec<RecordRow>,
}

/// A record key that is spread into this record, with the file path it comes from.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SpreadSource {
    pub key: String,
    /// File path (relative to project root) where the spread source record lives.
    /// Empty string if the file could not be determined.
    pub file: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RecordRow {
    pub key: String,
    pub actual_type: String,
    pub fields: Vec<FieldCell>,
    /// Field names that come from spread entries (not directly editable).
    pub spread_fields: Vec<String>,
    /// Records spread into this record (e.g. `...&base_item` → [{key:"base_item", file:"..."}]).
    pub spread_sources: Vec<SpreadSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct FieldCell {
    pub name: String,
    pub value: FieldValue,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "kind")]
#[ts(export)]
pub enum FieldValue {
    Null,
    Bool { v: bool },
    Int { v: f64 },
    Float { v: f64 },
    Str { v: String },
    Enum { enum_name: String, variant: String, int_value: f64 },
    Object { actual_type: String, fields: Vec<FieldCell> },
    Ref { target_type: String, target_key: String, target_file: Option<String> },
    Array { items: Vec<FieldValue> },
    Dict { entries: Vec<DictEntry> },
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DictEntry {
    pub key: DictKey,
    pub value: FieldValue,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "kind")]
#[ts(export)]
pub enum DictKey {
    Str { v: String },
    Int { v: f64 },
    Enum { enum_name: String, variant: String, int_value: f64 },
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "kind")]
#[ts(export)]
pub enum FieldPathSegment {
    Field { name: String },
    Index { i: usize },
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct GraphData {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct GraphNode {
    pub id: String,
    pub key: String,
    pub actual_type: String,
    pub file_path: String,
    pub in_focus_file: bool,
    pub is_collapsed: bool,
    pub fields: Vec<FieldCell>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct GraphEdge {
    pub source: String,
    pub target: String,
    pub field_path: String,
}

/// Lightweight summary of a record for the command palette / jump-to-record.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RecordBrief {
    pub key: String,
    pub actual_type: String,
    pub file_path: String,
}

/// Schema information for a single field on a type.
/// Used to enable schema-aware editing (e.g. creating a nullable Object value).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct FieldSchema {
    pub name: String,
    /// Human-readable type string (e.g. "Stats", "Stats?", "int[]")
    pub type_str: String,
    /// If this field's type is `T?` where T is an Object type, this is T.
    /// Used by the UI to offer a "Create T object" button for null values.
    pub nullable_object_type: Option<String>,
    /// Whether this field has a default value (implies it's optional in practice).
    pub has_default: bool,
}
