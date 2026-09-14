//! graph wire types.
use coflow_runtime::RecordCoordinate;
use serde::{Deserialize, Serialize};
#[cfg(feature = "ts-export")]
use ts_rs::TS;
use super::records::{FieldCell, FieldDiagnostic};

#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
pub struct GraphData {
    pub revision: u32,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub available_fields: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphQuery {
    pub file_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depth: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
pub struct GraphNode {
    pub coordinate: RecordCoordinate,
    pub file_path: String,
    pub in_focus_file: bool,
    pub is_collapsed: bool,
    pub fields: Vec<FieldCell>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub field_diagnostics: Vec<FieldDiagnostic>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostic_severity: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
pub struct GraphEdge {
    pub source: RecordCoordinate,
    pub target: RecordCoordinate,
    pub field_path: String,
}

/// Wire-friendly handle on a record the editor can jump to (a `Ref`'s
/// resolved target). Carries the coordinate + the file the record lives
/// in so the front-end can navigate without a follow-up query.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
pub struct RefTarget {
    pub short_name: Option<String>,
    pub coordinate: RecordCoordinate,
    pub file_path: String,
}
