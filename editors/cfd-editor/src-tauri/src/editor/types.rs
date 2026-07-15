//! Wire types serialized to the editor frontend.
//!
//! After the core-types refactor (spec 17), the editor stops re-defining
//! "value", "path segment", "dict key" and friends; those are imported
//! from `coflow-data-model` / `coflow-api` / `coflow-runtime` and shipped
//! straight to the front-end. The types that *remain* here are
//! composition views — `RecordRow`, `FieldCell`, `FieldAnnotation`,
//! `ProjectSnapshot`, ... — that bundle core data with editor-specific
//! derived metadata (file hints, enum int values, spread info, ...).

use coflow_api::{FlatDiagnostic, WriterCapabilities};
use coflow_data_model::{CfdDictKey, CfdRecord, CfdValue};
use coflow_runtime::{FileTreeNode, RecordCoordinate};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[cfg(feature = "ts-export")]
use ts_rs::TS;

/// Structured error returned by `SessionStore` methods.
///
/// Wire-shape: a discriminator (`kind`), a human-readable `message`, and an
/// optional list of structured `diagnostics` mirroring the same payload the
/// front-end already renders for build/load/check errors. The front-end can
/// route by `kind`, show `message` in a banner, and inject `diagnostics`
/// into the diagnostics panel without doing any string parsing.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
pub struct EditorError {
    pub kind: EditorErrorKind,
    pub message: String,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub diagnostics: Vec<FlatDiagnostic>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
#[serde(rename_all = "snake_case")]
pub enum EditorErrorKind {
    Session,
    Project,
    Write,
    NotFound,
    Other,
}

impl EditorError {
    #[must_use]
    pub fn new(kind: EditorErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            diagnostics: Vec::new(),
        }
    }

    #[must_use]
    pub fn session(message: impl Into<String>) -> Self {
        Self::new(EditorErrorKind::Session, message)
    }

    #[must_use]
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(EditorErrorKind::NotFound, message)
    }

    #[must_use]
    pub fn project(message: impl Into<String>) -> Self {
        Self::new(EditorErrorKind::Project, message)
    }

    #[must_use]
    pub fn write(message: impl Into<String>) -> Self {
        Self::new(EditorErrorKind::Write, message)
    }

    #[must_use]
    pub fn other(message: impl Into<String>) -> Self {
        Self::new(EditorErrorKind::Other, message)
    }

    #[must_use]
    pub fn with_diagnostics(mut self, diagnostics: Vec<FlatDiagnostic>) -> Self {
        self.diagnostics = diagnostics;
        self
    }
}

impl std::fmt::Display for EditorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for EditorError {}

impl From<&str> for EditorError {
    fn from(message: &str) -> Self {
        Self::other(message)
    }
}

impl From<String> for EditorError {
    fn from(message: String) -> Self {
        Self::other(message)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
pub struct ProjectSnapshot {
    pub session_id: u32,
    pub revision: u32,
    pub project_root: String,
    pub file_tree: Vec<FileTreeNode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_source_file: Option<String>,
    pub diagnostics: Vec<FlatDiagnostic>,
}

#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
pub struct FileRecords {
    pub revision: u32,
    pub file_path: String,
    pub type_names: Vec<String>,
    pub columns: Vec<RecordColumn>,
    pub records: Vec<RecordRow>,
    pub capabilities: WriterCapabilities,
}

/// A top-level field column available in a file/type table.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
pub struct RecordColumn {
    pub name: String,
    pub type_names: Vec<String>,
    pub max_summary_len: usize,
}

/// One top-level record's view inside a file.
///
/// The record's stable identity is its `(actual_type, key)` coordinate.
/// `display_path` repeats the file path for hosts that already have a row.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
pub struct RecordRow {
    pub coordinate: RecordCoordinate,
    pub display_path: String,
    pub fields: Vec<FieldCell>,
    pub field_index: BTreeMap<String, usize>,
    pub field_summaries: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub field_diagnostics: Vec<FieldDiagnostic>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostic_severity: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
pub struct FieldDiagnostic {
    pub severity: String,
    pub field_path: String,
    pub message: String,
}

/// One cell in a record row.
///
/// `value` is the authoritative `CfdValue`, shipped straight from the core
/// model. `annotation` carries spread, ref-target, and enum metadata.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
pub struct FieldCell {
    pub name: String,
    pub value: CfdValue,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotation: Option<FieldAnnotation>,
}

/// Editor-only derived metadata for a single cell.
///
/// - `spread_info`: cell came from a `...spread`; carries the source coordinate
///   and a file-path hint for jump-to-source.
/// - `ref_target_file`: project-relative file path of the record this cell
///   refers to. Only meaningful when `value` is a `CfdValue::Ref`.
/// - `enum_int_value`: integer backing the variant when `value` is a
///   `CfdValue::Enum`. The variant name lives on the value itself; the
///   integer is convenient for displays / filtering.
/// - `declared_type`: the schema type declared for this field, formatted for
///   display and for collection element type derivation in the UI.
/// - `ref_target_type`: direct reference target type for scalar ref cells.
/// - `enum_type`: the enum type name when this field's declared type resolves
///   to an enum. Set regardless of value kind so the front-end can show an
///   enum dropdown even for `null` cells.
/// - `nullable`: true when the declared type outer-wraps a `?`, so the UI
///   can offer a "clear to null" option in dropdowns.
/// - `children`: nested annotations for object fields, array items, or dict
///   values. Keys are field names, zero-based array indexes, or dict-key text.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
pub struct FieldAnnotation {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spread_info: Option<SpreadInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ref_target_file: Option<String>,
    #[serde(
        default,
        with = "coflow_data_model::serde_i64::option",
        skip_serializing_if = "Option::is_none"
    )]
    pub enum_int_value: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declared_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ref_target_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enum_type: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub nullable: bool,
    /// True when this cell is exposed for inspection but cannot be edited.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub read_only: bool,
    /// Template annotation for elements of an array/dict field. Carries the
    /// declared element type (and derived ref/enum/nullable metadata) so the
    /// editor doesn't have to re-parse `declared_type` strings when adding a
    /// new element or when the collection is empty. `None` for non-collection
    /// fields.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item_annotation: Option<Box<Self>>,
    /// Concrete types that could occupy this field when the declared type is
    /// an abstract object. Empty for non-polymorphic fields. The editor uses
    /// this to expose a type-switch control on object cells and to prompt for
    /// a concrete type when materializing a null polymorphic field.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub polymorphic_types: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub children: BTreeMap<String, Self>,
}

impl FieldAnnotation {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.spread_info.is_none()
            && self.ref_target_file.is_none()
            && self.enum_int_value.is_none()
            && self.declared_type.is_none()
            && self.ref_target_type.is_none()
            && self.enum_type.is_none()
            && !self.nullable
            && !self.read_only
            && self.item_annotation.is_none()
            && self.polymorphic_types.is_empty()
            && self.children.is_empty()
    }
}

/// Source record coordinate of a spread-inherited cell, plus the field
/// path within the source record so the UI can render source attribution.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
pub struct SpreadInfo {
    pub source: RecordCoordinate,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_record_file: Option<String>,
    pub source_field_path: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
pub struct WriteFieldOutcome {
    pub revision: u32,
    pub row: RecordRow,
    pub diagnostics: Vec<FlatDiagnostic>,
    /// Value at the target path before the write. Captured by the backend
    /// from engine state so undo does not depend on a stale front-end cache.
    #[serde(default)]
    pub old_value: Option<CfdValue>,
    /// Value at the target path after the write. Collection edits are built
    /// in the backend, so the frontend uses this authoritative value for
    /// undo/redo instead of reconstructing the collection mutation.
    #[serde(default)]
    pub new_value: Option<CfdValue>,
    #[serde(default)]
    pub affected_files: Vec<String>,
    /// `Some(new_coordinate)` when the write changed the host record's `id`
    /// field. The front-end refreshes any caches keyed by the old coordinate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub renamed: Option<RecordCoordinate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CollectionEdit {
    ArrayAppend {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts-export", ts(optional))]
        value: Option<CfdValue>,
    },
    ArrayRemove {
        index: usize,
    },
    ArrayMove {
        from: usize,
        to: usize,
    },
    DictInsert {
        key: CfdDictKey,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts-export", ts(optional))]
        value: Option<CfdValue>,
    },
    DictRemove {
        key: CfdDictKey,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
pub struct RenameRecordOutcome {
    pub revision: u32,
    pub row: RecordRow,
    pub diagnostics: Vec<FlatDiagnostic>,
    pub renamed: RecordCoordinate,
    pub affected_files: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
pub struct InsertRecordOutcome {
    pub revision: u32,
    pub file_records: FileRecords,
    pub diagnostics: Vec<FlatDiagnostic>,
    pub affected_files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
pub struct CreateRecordDraft {
    pub actual_type: String,
    pub fields: Vec<CreateRecordFieldDraft>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
pub struct CreateRecordFieldDraft {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<CfdValue>,
    pub source: CreateFieldSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required: Option<CreateRequiredInput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotation: Option<FieldAnnotation>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
#[serde(rename_all = "snake_case")]
pub enum CreateFieldSource {
    SchemaDefault,
    TypeSeed,
    RequiredInput,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CreateRequiredInput {
    Ref {
        target_type: String,
    },
    AbstractObject {
        expected_type: String,
        concrete_types: Vec<String>,
    },
    RecursiveObject {
        type_name: String,
    },
    Unsupported {
        message: String,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
pub struct DeleteRecordOutcome {
    pub revision: u32,
    pub file_records: FileRecords,
    pub diagnostics: Vec<FlatDiagnostic>,
    pub affected_files: Vec<String>,
    /// Authoritative snapshot of the deleted record so the front-end's undo
    /// can re-insert it. `None` only when the record was missing before
    /// deletion (defensive — should not happen in normal flows).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deleted_snapshot: Option<DeletedRecordSnapshot>,
}

#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
pub struct DeletedRecordSnapshot {
    pub record: CfdRecord,
    pub display_path: String,
}

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
#[cfg_attr(feature = "ts-export", derive(TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
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
    pub coordinate: RecordCoordinate,
    pub file_path: String,
}
