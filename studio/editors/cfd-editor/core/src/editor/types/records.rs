//! record row wire types.
use coflow_project::{CfdRecord, CfdValue, DimensionValueState, RecordCoordinate};
pub use coflow_project::{CreateFieldSource, CreateRequiredInput};
use coflow_project::{FlatDiagnostic, WriterCapabilities};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[cfg(feature = "ts-export")]
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
pub struct DimensionFileRecords {
    pub revision: u32,
    pub file_path: String,
    pub dimension: String,
    pub display_name: String,
    pub variants: Vec<String>,
    pub rows: Vec<DimensionFileRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
pub struct DimensionFileRow {
    pub coordinate: RecordCoordinate,
    pub field: String,
    pub owner_file_path: String,
    pub default_value: CfdValue,
    pub values: BTreeMap<String, DimensionValueState>,
}

#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
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
pub struct RecordRow {
    pub coordinate: RecordCoordinate,
    pub display_path: String,
    /// Zero-based position inside the record's physical file or table sheet.
    pub container_index: usize,
    pub container_size: usize,
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
pub struct FieldDiagnostic {
    pub severity: String,
    pub field_path: String,
    pub message: String,
}

/// One cell in a record row.
///
/// `value` is the authoritative `CfdValue`, shipped straight from the core
/// model. `annotation` carries ref-target and enum metadata.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
pub struct FieldCell {
    pub name: String,
    pub value: CfdValue,
    /// The source record does not currently contain this declared field.
    /// `value` is an editor seed and is not part of the runtime data model
    /// until the user commits it.
    pub missing: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotation: Option<FieldAnnotation>,
}

/// Editor-only derived metadata for a single cell.
///
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
pub struct FieldAnnotation {
    /// Optional schema display name for this field; storage continues to use the field name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts-export", ts(optional))]
    pub label: Option<String>,
    /// Optional schema documentation shown by the editor as contextual help.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts-export", ts(optional))]
    pub description: Option<String>,
    #[serde(
        default,
        with = "coflow_project::serde_i64::option",
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
    pub enum_is_flag: bool,
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
    /// 字典键的 schema 模板。新增首项时必须依据它选择键编辑器，不能从已有数据猜测。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts-export", ts(optional))]
    pub key_annotation: Option<Box<Self>>,
    /// Concrete types that could occupy this field when the declared type is
    /// an abstract object. Empty for non-polymorphic fields. The editor uses
    /// this to expose a type-switch control on object cells and to prompt for
    /// a concrete type when materializing a null polymorphic field.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub polymorphic_types: Vec<String>,
    /// Concrete object type when schema context determines one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object_type: Option<String>,
    /// Direct object fields in inherited schema declaration order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub field_order: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub children: BTreeMap<String, Self>,
}

/// Stable enum variant identity plus schema-provided presentation metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
pub struct EnumVariantOption {
    pub name: String,
    #[serde(with = "coflow_project::serde_i64")]
    #[cfg_attr(feature = "ts-export", ts(type = "bigint"))]
    pub value: i64,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

impl FieldAnnotation {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.label.is_none()
            && self.description.is_none()
            && self.enum_int_value.is_none()
            && self.declared_type.is_none()
            && self.ref_target_type.is_none()
            && self.enum_type.is_none()
            && !self.enum_is_flag
            && !self.nullable
            && !self.read_only
            && self.item_annotation.is_none()
            && self.key_annotation.is_none()
            && self.polymorphic_types.is_empty()
            && self.object_type.is_none()
            && self.field_order.is_empty()
            && self.children.is_empty()
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
pub struct WriteFieldOutcome {
    pub changes: EditorChangeSet,
    pub revision: u32,
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
pub struct BatchWriteFieldEditOutcome {
    pub coordinate: RecordCoordinate,
    pub final_coordinate: RecordCoordinate,
    pub field_path: Vec<coflow_project::CfdPathSegment>,
    #[serde(default)]
    pub old_value: Option<CfdValue>,
    #[serde(default)]
    pub new_value: Option<CfdValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
pub struct BatchWriteFieldInput {
    pub coordinate: RecordCoordinate,
    pub field_path: Vec<coflow_project::CfdPathSegment>,
    pub new_value: CfdValue,
}

#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
pub struct BatchWriteFieldOutcome {
    pub changes: EditorChangeSet,
    pub revision: u32,
    pub edits: Vec<BatchWriteFieldEditOutcome>,
    pub diagnostics: Vec<FlatDiagnostic>,
    #[serde(default)]
    pub affected_files: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
pub struct WriteDimensionValueOutcome {
    pub changes: EditorChangeSet,
    pub revision: u32,
    pub coordinate: coflow_project::DimensionValueCoordinate,
    pub old_value: DimensionValueState,
    pub new_value: DimensionValueState,
    pub diagnostics: Vec<FlatDiagnostic>,
    pub affected_files: Vec<String>,
}

pub use coflow_project::CollectionEdit;

#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
pub struct RenameRecordOutcome {
    pub changes: EditorChangeSet,
    pub revision: u32,
    pub diagnostics: Vec<FlatDiagnostic>,
    pub renamed: RecordCoordinate,
    pub affected_files: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
pub struct InsertRecordOutcome {
    pub changes: EditorChangeSet,
    pub revision: u32,
    pub diagnostics: Vec<FlatDiagnostic>,
    pub affected_files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
pub struct CreateRecordDraft {
    pub actual_type: String,
    pub fields: Vec<CreateRecordFieldDraft>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
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

#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
pub struct DeleteRecordOutcome {
    pub changes: EditorChangeSet,
    pub revision: u32,
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
pub struct ReorderRecordsOutcome {
    pub changes: EditorChangeSet,
    pub revision: u32,
    pub diagnostics: Vec<FlatDiagnostic>,
    pub affected_files: Vec<String>,
    #[serde(default)]
    pub old_index: Option<usize>,
    #[serde(default)]
    pub new_index: Option<usize>,
}

#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
pub struct DeletedRecordSnapshot {
    pub record: CfdRecord,
    pub display_path: String,
}

#[cfg(test)]
mod tests {
    use super::EnumVariantOption;

    #[test]
    fn enum_variant_option_serializes_missing_metadata_as_null() {
        let value = serde_json::to_value(EnumVariantOption {
            name: "active".to_owned(),
            value: 1,
            label: None,
            description: None,
        });

        assert!(value.is_ok());
        assert_eq!(
            value.as_ref().ok().and_then(|value| value.get("value")),
            Some(&serde_json::Value::String("1".to_string()))
        );
        assert_eq!(
            value.as_ref().ok().and_then(|value| value.get("label")),
            Some(&serde_json::Value::Null)
        );
        assert_eq!(
            value
                .as_ref()
                .ok()
                .and_then(|value| value.get("description")),
            Some(&serde_json::Value::Null)
        );
    }
}

/// 与一次提交绑定的文件增量；order 是权威顺序，records 只携带变化的行。
#[derive(Debug, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
pub struct EditorChangeSet {
    pub base_revision: u32,
    pub revision: u32,
    pub files: Vec<FileRecordsPatch>,
}

#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
pub struct FileRecordsPatch {
    pub data: FileRecords,
    pub order: Vec<RecordCoordinate>,
}
