use std::collections::BTreeMap;

use coflow_api::{DiagnosticSet, FlatDiagnostic};
use coflow_cft::{DimensionName, FieldName, RecordKey, TypeName, VariantName};
use coflow_data_model::{CfdPath, CfdPathSegment, CfdValue};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{RecordCoordinate, WriteOutcome};

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
        #[serde(default)]
        sheet: Option<String>,
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
#[serde(rename_all = "snake_case")]
pub enum CreateFieldSource {
    SchemaDefault,
    TypeSeed,
    RequiredInput,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone)]
pub(crate) enum PreparedMutationOp {
    InsertRecord {
        file: String,
        sheet: Option<String>,
        actual_type: TypeName,
        key: RecordKey,
        fields: BTreeMap<String, CfdValue>,
    },
    SetField {
        record: RecordCoordinate,
        write_record: RecordCoordinate,
        write_file: String,
        path: Vec<coflow_api::WriteFieldPathSegment>,
        value: CfdValue,
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
    FoldedSetField {
        record: RecordCoordinate,
        write_file: String,
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
    /// Provider diagnostics followed by diagnostics from the published generation.
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
    use coflow_api::{Diagnostic, Label, Severity, SourceLocation};

    use super::MutationFailedOp;

    #[test]
    fn mutation_failure_keeps_structured_diagnostics_after_flattening() {
        let primary = Label {
            location: SourceLocation::TableCell {
                path: "items.csv".into(),
                sheet: Some("items".to_string()),
                row: 2,
                column: 3,
            },
            message: Some("primary".to_string()),
        };
        let related = Label {
            location: SourceLocation::TableCell {
                path: "items.csv".into(),
                sheet: Some("items".to_string()),
                row: 4,
                column: 3,
            },
            message: Some("related".to_string()),
        };
        let failure = MutationFailedOp::from_diagnostics(
            0,
            "set_field",
            coflow_api::DiagnosticSet::one(Diagnostic {
                code: "TEST-STRUCTURED".to_string(),
                stage: "WRITE".to_string(),
                severity: Severity::Warning,
                message: "structured diagnostic".to_string(),
                primary: Some(primary.clone()),
                related: vec![related.clone()],
            }),
        );

        assert_eq!(failure.diagnostics[0].severity, "warning");
        let structured = failure.into_source_diagnostics();
        assert_eq!(structured.diagnostics[0].severity, Severity::Warning);
        assert_eq!(structured.diagnostics[0].primary, Some(primary));
        assert_eq!(structured.diagnostics[0].related, vec![related]);
    }
}
