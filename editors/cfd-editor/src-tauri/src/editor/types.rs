//! Wire types serialized to the editor frontend.
//!
//! These mirror `editors/cfd-editor/frontend/src/bindings/index.ts`.

use serde::{Deserialize, Serialize};

/// Structured error returned by `SessionStore` methods.
///
/// Wire-shape: a discriminator (`kind`), a human-readable `message`, and an
/// optional list of structured `diagnostics` mirroring the same payload the
/// front-end already renders for build/load/check errors. The front-end can
/// route by `kind`, show `message` in a banner, and inject `diagnostics`
/// into the diagnostics panel without doing any string parsing.
#[derive(Debug, Clone, Serialize)]
pub struct EditorError {
    pub kind: EditorErrorKind,
    pub message: String,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub diagnostics: Vec<DiagnosticItem>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EditorErrorKind {
    /// Session lookup / poisoning / lifecycle failures.
    Session,
    /// Project parsing or schema compilation failed before any data could be
    /// loaded.
    Project,
    /// A writer rejected an edit (origin mismatch, schema-invalid value,
    /// transport error, ...).
    Write,
    /// A precondition for the requested operation was not met (record not
    /// found, file not in this project, ...).
    NotFound,
    /// Anything else.
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
    pub fn with_diagnostics(mut self, diagnostics: Vec<DiagnosticItem>) -> Self {
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

#[derive(Debug, Serialize)]
pub struct ProjectSnapshot {
    pub session_id: u32,
    pub project_root: String,
    pub file_tree: Vec<FileTreeNode>,
    /// Flattened diagnostics for the initial wire snapshot. Each item
    /// already carries its `stage` (`SCHEMA`, `LOAD`, `CHECK`, ...) so the
    /// front-end can group/filter without keeping a separate index.
    pub diagnostics: Vec<DiagnosticItem>,
}

#[derive(Debug, Serialize)]
pub struct FileTreeNode {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub in_sources: bool,
    pub children: Vec<Self>,
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
    /// What the front-end is allowed to do with this file. Driven by the
    /// `WriterCapabilities` of the registered writer for this source.
    pub capabilities: SourceCapabilities,
}

/// Per-source capabilities surfaced to the editor UI. The front-end greys
/// out actions whose flag is `false`.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Copy, Serialize)]
pub struct SourceCapabilities {
    pub provider_id: &'static str,
    pub can_edit_field: bool,
    pub can_edit_key: bool,
    pub can_insert_record: bool,
    pub can_delete_record: bool,
    pub is_remote: bool,
}

impl SourceCapabilities {
    #[must_use]
    pub const fn read_only(provider_id: &'static str) -> Self {
        Self {
            provider_id,
            can_edit_field: false,
            can_edit_key: false,
            can_insert_record: false,
            can_delete_record: false,
            is_remote: false,
        }
    }

    /// Capability profile for localization CSVs: only field edits are
    /// allowed, and even those are further gated by `FieldCell.read_only`
    /// on the per-cell level (id / default columns stay read-only).
    #[must_use]
    pub const fn localization() -> Self {
        Self {
            provider_id: "localization",
            can_edit_field: true,
            can_edit_key: false,
            can_insert_record: false,
            can_delete_record: false,
            is_remote: false,
        }
    }

    #[must_use]
    pub const fn from_writer(
        provider_id: &'static str,
        capabilities: coflow_api::WriterCapabilities,
    ) -> Self {
        Self {
            provider_id,
            can_edit_field: capabilities.can_edit_field,
            can_edit_key: capabilities.can_edit_key,
            can_insert_record: capabilities.can_insert_record,
            can_delete_record: capabilities.can_delete_record,
            is_remote: capabilities.is_remote,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct RecordRow {
    pub key: String,
    pub actual_type: String,
    pub fields: Vec<FieldCell>,
}

/// Result of a successful `write_field` Tauri command.
///
/// Bundles the refreshed row with the project's diagnostics post-rebuild so the
/// front-end can refresh the diagnostics panel without issuing a follow-up
/// query.
///
/// Diagnostics are returned **flattened across stages** (`schema + load +
/// check`) — same shape the front-end already gets from `load_project`.
/// The check stage is the one that typically changes after a write; the
/// other stages stay stable until the project is fully rebuilt.
#[derive(Debug, Serialize)]
pub struct WriteFieldOutcome {
    pub row: RecordRow,
    pub diagnostics: Vec<DiagnosticItem>,
}

/// Result of `insert_record`: the refreshed list of records for the host
/// file plus the project's diagnostics post-rebuild.
#[derive(Debug, Serialize)]
pub struct InsertRecordOutcome {
    pub file_records: FileRecords,
    pub diagnostics: Vec<DiagnosticItem>,
}

/// Result of `delete_record`: the refreshed list of records for the host
/// file plus the project's diagnostics post-rebuild.
#[derive(Debug, Serialize)]
pub struct DeleteRecordOutcome {
    pub file_records: FileRecords,
    pub diagnostics: Vec<DiagnosticItem>,
    /// Snapshot of the deleted record as the front-end's `FieldValue::Object`
    /// shape. Front-end persists this in its undo stack so a later undo can
    /// re-insert the record with full fidelity (including spread/ref
    /// metadata) without depending on a still-warm `fileDataCache` row.
    ///
    /// `actual_type` mirrors the deleted record's concrete type. Both fields
    /// are `None` only when the record could not be located before
    /// deletion (defensive — should not happen in normal flows).
    pub deleted_snapshot: Option<FieldValue>,
    pub deleted_actual_type: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FieldCell {
    pub name: String,
    pub value: FieldValue,
    /// True when this field comes from a `...spread` expansion (any
    /// nesting level). Mirrors `spread_info.is_some()` for legacy callers
    /// that only need a boolean — new code should consult `spread_info`.
    #[serde(default, skip_serializing_if = "is_false")]
    pub is_spread: bool,
    /// True when this cell must not be edited by the user. Set by the
    /// localization wire path for the `id` and `default` columns; not
    /// emitted for regular records.
    #[serde(default, skip_serializing_if = "is_false")]
    pub read_only: bool,
    /// Where the value of this cell originally came from, when it was
    /// imported via a `...spread` expansion. `None` means the cell is
    /// declared directly on the host record. The editor uses this to:
    /// - render the cell as inherited (greyed background, source tooltip),
    /// - offer a "jump to source" affordance,
    /// - decide write semantics: by default an edit creates a local
    ///   override in the host record's source rather than mutating the
    ///   spread origin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spread_info: Option<SpreadInfo>,
}

#[allow(clippy::trivially_copy_pass_by_ref)]
const fn is_false(b: &bool) -> bool {
    !*b
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(clippy::struct_field_names)]
pub struct SpreadInfo {
    /// Record key the value was inherited from.
    pub source_record_key: String,
    /// Concrete type of the source record — useful for rendering the
    /// jump-to-source link (`@Type.key`).
    pub source_record_type: String,
    /// Project-relative file path of the source record, if known. Front-end
    /// uses this to navigate; absent for synthetic / inline sources.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_record_file: Option<String>,
    /// Path inside the source record that this cell mirrors. Empty when
    /// the spread is at the same nesting level as the source field. The
    /// editor concatenates this with `source_record_key` to render
    /// `Source.Key.path.to.field`.
    pub source_field_path: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
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
        items: Vec<Self>,
    },
    Dict {
        entries: Vec<DictEntry>,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DictEntry {
    pub key: DictKey,
    pub value: FieldValue,
}

#[derive(Debug, Serialize, Deserialize)]
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

#[derive(Debug, Deserialize)]
#[serde(tag = "kind")]
pub enum FieldPathSegment {
    /// Field-name segment. Also used for dict keys: the parser stores dict
    /// entries as Block fields whose `name` is the AST-form key (string keys
    /// without quotes, ints as their digit form, enum variants as identifier).
    #[serde(rename = "field")]
    Field { name: String },
    #[serde(rename = "index")]
    Index { i: usize },
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
