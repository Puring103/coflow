use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::api::{DiagnosticSet, FlatDiagnostic};
use crate::data_model::{CfdPath, CfdPathSegment, CfdValue};
use coflow_language::cft::{DimensionName, FieldName, RecordKey, TypeName, VariantName};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{RecordCoordinate, WriteOutcome};

/// An application-owned project file that must be published with a mutation.
///
/// The runtime treats this as opaque bytes. The expected contents are checked
/// immediately before publication so an external edit cannot be overwritten.
#[derive(Debug, Clone)]
pub struct ProjectFileUpdate {
    pub(crate) path: PathBuf,
    pub(crate) expected: Option<Vec<u8>>,
    pub(crate) contents: Vec<u8>,
}

impl ProjectFileUpdate {
    #[must_use]
    pub fn new(path: impl Into<PathBuf>, expected: Option<Vec<u8>>, contents: Vec<u8>) -> Self {
        Self {
            path: path.into(),
            expected,
            contents,
        }
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MutationRequest {
    #[serde(default = "default_true")]
    pub stop_on_write_error: bool,
    pub ops: Vec<MutationOp>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum MutationOp {
    InsertRecord {
        file: String,
        #[serde(rename = "type")]
        actual_type: String,
        key: String,
        #[serde(default)]
        fields: MutationFields,
        #[serde(default)]
        materialization: DefaultMaterialization,
    },
    SetField {
        record: RecordCoordinate,
        #[serde(default)]
        file: Option<String>,
        path: Vec<CfdPathSegment>,
        value: MutationValue,
    },
    UnsetField {
        record: RecordCoordinate,
        #[serde(default)]
        file: Option<String>,
        path: Vec<CfdPathSegment>,
    },
    SetDimensionValue {
        coordinate: DimensionValueCoordinate,
        #[serde(default)]
        expected: DimensionValueExpectation,
        value: MutationValue,
    },
    ClearDimensionValue {
        coordinate: DimensionValueCoordinate,
        #[serde(default)]
        expected: DimensionValueExpectation,
    },
    RenameRecord {
        record: RecordCoordinate,
        #[serde(default)]
        file: Option<String>,
        new_key: String,
    },
    DeleteRecord {
        record: RecordCoordinate,
        #[serde(default)]
        file: Option<String>,
    },
    SwapRecords {
        first: RecordCoordinate,
        second: RecordCoordinate,
        #[serde(default)]
        file: Option<String>,
    },
    MoveRecord {
        record: RecordCoordinate,
        target_index: usize,
        #[serde(default)]
        file: Option<String>,
    },
    TransferRecord {
        record: RecordCoordinate,
        destination_file: String,
        target_index: usize,
        #[serde(default)]
        source_file: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
pub struct DimensionValueCoordinate {
    #[cfg_attr(feature = "ts-export", ts(type = "string"))]
    pub actual_type: TypeName,
    #[cfg_attr(feature = "ts-export", ts(type = "string"))]
    pub record_key: RecordKey,
    #[cfg_attr(feature = "ts-export", ts(type = "string"))]
    pub field: FieldName,
    #[cfg_attr(feature = "ts-export", ts(type = "string"))]
    pub dimension: DimensionName,
    #[cfg_attr(feature = "ts-export", ts(type = "string"))]
    pub variant: VariantName,
    #[serde(default)]
    pub path: Vec<CfdPathSegment>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DimensionSourceCoordinate {
    pub source_type: TypeName,
    pub source_key: RecordKey,
    pub field: FieldName,
    pub dimension: DimensionName,
    pub variant: VariantName,
    pub path: CfdPath,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum DimensionValueExpectation {
    #[default]
    Any,
    Missing,
    Value(MutationValue),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum MutationValue {
    Json(Value),
    Cfd(CfdValue),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum MutationFields {
    #[default]
    Empty,
    Json(BTreeMap<String, Value>),
    Cfd(BTreeMap<String, CfdValue>),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DefaultMaterialization {
    #[default]
    Minimal,
    EditableShape,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreateRecordDraft {
    pub actual_type: String,
    pub fields: Vec<CreateRecordFieldDraft>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreateRecordFieldDraft {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<CfdValue>,
    pub source: CreateFieldSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required: Option<CreateRequiredInput>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
#[serde(rename_all = "snake_case")]
pub enum CreateFieldSource {
    SchemaDefault,
    TypeDefault,
    RequiredInput,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
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
    Unsupported {
        message: String,
    },
}

#[derive(Debug, Clone)]
pub(crate) enum PreparedMutationOp {
    InsertRecord {
        file: String,
        actual_type: TypeName,
        key: RecordKey,
        fields: BTreeMap<String, CfdValue>,
    },
    SetField {
        record: RecordCoordinate,
        write_record: RecordCoordinate,
        write_file: String,
        path: Vec<crate::api::WriteFieldPathSegment>,
        value: CfdValue,
        materialized_top_level: Option<CfdValue>,
    },
    UnsetField {
        record: RecordCoordinate,
        write_record: RecordCoordinate,
        write_file: String,
        path: Vec<CfdPathSegment>,
    },
    WriteDimensionValue {
        record: RecordCoordinate,
        coordinate: DimensionSourceCoordinate,
        new_value: Option<CfdValue>,
        write_file: String,
    },
    RenameRecord {
        record: RecordCoordinate,
        new_key: RecordKey,
        report_file: Option<String>,
    },
    DeleteRecord {
        record: RecordCoordinate,
        report_file: Option<String>,
    },
    SwapRecords {
        first: RecordCoordinate,
        second: RecordCoordinate,
        report_file: String,
    },
    MoveRecord {
        record: RecordCoordinate,
        target_index: usize,
        report_file: String,
    },
    TransferRecord {
        record: RecordCoordinate,
        destination_file: String,
        target_index: usize,
    },
    FoldedSetField {
        record: RecordCoordinate,
        write_file: String,
        path: CfdPath,
    },
    FoldedRenameRecord {
        old_record: RecordCoordinate,
        new_record: RecordCoordinate,
        write_file: String,
    },
    FoldedDeleteRecord {
        record: RecordCoordinate,
        write_file: String,
    },
    CancelledInsert {
        record: RecordCoordinate,
        write_file: String,
    },
}

impl PreparedMutationOp {
    pub(crate) fn report_metadata(
        &self,
    ) -> (&'static str, Option<RecordCoordinate>, Option<String>) {
        match self {
            Self::InsertRecord {
                file,
                actual_type,
                key,
                ..
            } => (
                "insert_record",
                Some(RecordCoordinate::new(actual_type.clone(), key.clone())),
                Some(file.clone()),
            ),
            Self::CancelledInsert { record, write_file } => (
                "insert_record",
                Some(record.clone()),
                Some(write_file.clone()),
            ),
            Self::SetField {
                record, write_file, ..
            }
            | Self::FoldedSetField {
                record, write_file, ..
            } => ("set_field", Some(record.clone()), Some(write_file.clone())),
            Self::UnsetField {
                record,
                write_file,
                ..
            } => ("unset_field", Some(record.clone()), Some(write_file.clone())),
            Self::WriteDimensionValue {
                record,
                new_value,
                write_file,
                ..
            } => (
                if new_value.is_some() {
                    "set_dimension_value"
                } else {
                    "clear_dimension_value"
                },
                Some(record.clone()),
                Some(write_file.clone()),
            ),
            Self::RenameRecord {
                record,
                new_key,
                report_file,
            } => (
                "rename_record",
                Some(RecordCoordinate::new(
                    record.actual_type.clone(),
                    new_key.clone(),
                )),
                report_file.clone(),
            ),
            Self::FoldedRenameRecord {
                new_record,
                write_file,
                ..
            } => (
                "rename_record",
                Some(new_record.clone()),
                Some(write_file.clone()),
            ),
            Self::DeleteRecord {
                record,
                report_file,
            } => ("delete_record", Some(record.clone()), report_file.clone()),
            Self::FoldedDeleteRecord { record, write_file } => (
                "delete_record",
                Some(record.clone()),
                Some(write_file.clone()),
            ),
            Self::SwapRecords {
                first, report_file, ..
            } => (
                "swap_records",
                Some(first.clone()),
                Some(report_file.clone()),
            ),
            Self::MoveRecord {
                record,
                report_file,
                ..
            } => (
                "move_record",
                Some(record.clone()),
                Some(report_file.clone()),
            ),
            Self::TransferRecord {
                record,
                destination_file,
                ..
            } => (
                "transfer_record",
                Some(record.clone()),
                Some(destination_file.clone()),
            ),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct MutationReport {
    pub write_ok: bool,
    pub check_ok: bool,
    /// Whether this request published a new project generation.
    pub generation_changed: bool,
    pub applied: Vec<MutationAppliedOp>,
    pub failed: Vec<MutationFailedOp>,
    /// Deduplicated project-facing source paths changed by the transaction.
    pub affected_files: Vec<String>,
    /// Deduplicated project-relative paths physically published by the transaction.
    ///
    /// Hosts use this broader set to distinguish their own writes from external
    /// filesystem changes. Unlike `affected_files`, it may contain non-CFD
    /// project files such as `coflow.enum.lock.json`.
    pub written_files: Vec<String>,
    /// Writer diagnostics followed by diagnostics from the published generation.
    pub diagnostics: Vec<FlatDiagnostic>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MutationAppliedOp {
    pub index: usize,
    pub op: String,
    pub record: Option<RecordCoordinate>,
    pub file: Option<String>,
    pub outcome: WriteOutcome,
}

#[derive(Debug, Clone, Serialize)]
pub struct MutationFailedOp {
    pub index: usize,
    pub op: String,
    pub diagnostics: Vec<FlatDiagnostic>,
    #[serde(skip)]
    source_diagnostics: DiagnosticSet,
}

impl MutationFailedOp {
    pub(super) fn from_diagnostics(
        index: usize,
        op: impl Into<String>,
        source_diagnostics: DiagnosticSet,
    ) -> Self {
        let diagnostics = source_diagnostics.flat_diagnostics();
        Self {
            index,
            op: op.into(),
            diagnostics,
            source_diagnostics,
        }
    }

    pub(crate) fn into_source_diagnostics(self) -> DiagnosticSet {
        self.source_diagnostics
    }
}

const fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use crate::api::{Diagnostic, Label, Severity, SourceLocation};

    use super::MutationFailedOp;

    #[test]
    fn mutation_failure_keeps_structured_diagnostics_after_flattening() {
        let primary = Label {
            location: SourceLocation::FileSpan {
                path: "items.cfd".into(),
                start_line: 1,
                start_character: 0,
                end_line: 1,
                end_character: 1,
            },
            message: Some("primary".to_string()),
        };
        let related = Label {
            location: SourceLocation::FileSpan {
                path: "items.cfd".into(),
                start_line: 3,
                start_character: 0,
                end_line: 3,
                end_character: 1,
            },
            message: Some("related".to_string()),
        };
        let failure = MutationFailedOp::from_diagnostics(
            0,
            "set_field",
            crate::api::DiagnosticSet::one(Diagnostic {
                code: "TEST-STRUCTURED".to_string(),
                stage: "WRITE".to_string(),
                severity: Severity::Warning,
                message: "structured diagnostic".to_string(),
                primary: Some(primary.clone()),
                related: vec![related.clone()],
                contexts: Vec::new(),
            }),
        );

        assert_eq!(failure.diagnostics[0].severity, "warning");
        let structured = failure.into_source_diagnostics();
        assert_eq!(structured.diagnostics[0].severity, Severity::Warning);
        assert_eq!(structured.diagnostics[0].primary, Some(primary));
        assert_eq!(structured.diagnostics[0].related, vec![related]);
    }
}
