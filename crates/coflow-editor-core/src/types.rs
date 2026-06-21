//! Wire types serialized to the editor frontend.
//!
//! These mirror `editors/cfd-editor/frontend/src/bindings/index.ts`.

use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct ProjectSnapshot {
    pub session_id: u32,
    pub project_root: String,
    pub file_tree: Vec<FileTreeNode>,
    pub diagnostics: Vec<DiagnosticItem>,
}

#[derive(Debug, Serialize)]
pub struct FileTreeNode {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub in_sources: bool,
    pub children: Vec<FileTreeNode>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticItem {
    pub severity: String,
    pub code: String,
    pub stage: String,
    pub message: String,
    pub file_path: Option<String>,
    pub record_key: Option<String>,
    pub field_path: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct FileRecords {
    pub file_path: String,
    pub type_names: Vec<String>,
    pub records: Vec<RecordRow>,
}

#[derive(Debug, Serialize)]
pub struct RecordRow {
    pub key: String,
    pub actual_type: String,
    pub fields: Vec<FieldCell>,
}

#[derive(Debug, Serialize)]
pub struct FieldCell {
    pub name: String,
    pub value: FieldValue,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind")]
pub enum FieldValue {
    Null,
    Bool {
        v: bool,
    },
    Int {
        v: i64,
    },
    Float {
        v: f64,
    },
    Str {
        v: String,
    },
    Enum {
        enum_name: String,
        variant: String,
        int_value: i64,
    },
    Object {
        actual_type: String,
        fields: Vec<FieldCell>,
    },
    Ref {
        target_type: String,
        target_key: String,
        target_file: Option<String>,
    },
    Array {
        items: Vec<FieldValue>,
    },
    Dict {
        entries: Vec<DictEntry>,
    },
}

#[derive(Debug, Serialize)]
pub struct DictEntry {
    pub key: DictKey,
    pub value: FieldValue,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind")]
pub enum DictKey {
    Str {
        v: String,
    },
    Int {
        v: i64,
    },
    Enum {
        enum_name: String,
        variant: String,
        int_value: i64,
    },
}

#[derive(Debug, Serialize)]
pub struct GraphData {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

#[derive(Debug, Serialize)]
pub struct GraphNode {
    pub id: String,
    pub key: String,
    pub actual_type: String,
    pub file_path: String,
    pub in_focus_file: bool,
    pub is_collapsed: bool,
    pub fields: Vec<FieldCell>,
}

#[derive(Debug, Serialize)]
pub struct GraphEdge {
    pub source: String,
    pub target: String,
    pub field_path: String,
}
