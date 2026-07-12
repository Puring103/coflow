use std::collections::BTreeMap;

use coflow_api::FlatDiagnostic;
use coflow_data_model::CfdPathSegment;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    DefaultMaterialization, MutationAppliedOp, MutationFailedOp, MutationFields, MutationOp,
    MutationReport, MutationRequest, MutationValue, RecordCoordinate, WriteOutcome,
    WriteProjectSession,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DataPatchRequest {
    #[serde(default = "default_true")]
    pub stop_on_write_error: bool,
    pub ops: Vec<DataPatchOp>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum DataPatchOp {
    InsertRecord {
        file: String,
        #[serde(default)]
        sheet: Option<String>,
        #[serde(rename = "type")]
        actual_type: String,
        key: String,
        #[serde(default)]
        fields: BTreeMap<String, Value>,
        #[serde(default)]
        materialization: DefaultMaterialization,
    },
    SetField {
        record: PatchRecordSelector,
        #[serde(default)]
        file: Option<String>,
        path: Vec<CfdPathSegment>,
        value: Value,
    },
    RenameRecord {
        record: PatchRecordSelector,
        #[serde(default)]
        file: Option<String>,
        new_key: String,
    },
    DeleteRecord {
        record: PatchRecordSelector,
        #[serde(default)]
        file: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PatchRecordSelector {
    #[serde(rename = "type")]
    pub actual_type: String,
    pub key: String,
}

pub type PatchPathSegment = CfdPathSegment;

#[derive(Debug, Clone, Serialize)]
pub struct DataPatchReport {
    pub write_ok: bool,
    pub check_ok: bool,
    pub applied: Vec<DataPatchAppliedOp>,
    pub failed: Vec<DataPatchFailedOp>,
    pub affected_files: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub remaining_ops: Vec<DataPatchOp>,
    pub diagnostics: Vec<FlatDiagnostic>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DataPatchAppliedOp {
    pub index: usize,
    pub op: String,
    pub record: Option<RecordCoordinate>,
    pub file: Option<String>,
    pub outcome: WriteOutcome,
}

#[derive(Debug, Clone, Serialize)]
pub struct DataPatchFailedOp {
    pub index: usize,
    pub op: String,
    pub diagnostics: Vec<FlatDiagnostic>,
}

const fn default_true() -> bool {
    true
}

impl WriteProjectSession {
    /// Apply a JSON data patch through the engine mutation API.
    ///
    /// This keeps the CLI-facing data patch DTOs as a thin adapter while all
    /// mutation semantics live in the shared engine path.
    ///
    pub fn apply_data_patch(&mut self, request: DataPatchRequest) -> DataPatchReport {
        let original_ops = request.ops.clone();
        let mutation_request = request.into_mutation_request();
        let mutation_report = self.apply_mutation(mutation_request);
        let remaining_ops =
            DataPatchRequest::remaining_after_failure(&original_ops, &mutation_report);
        mutation_report.into_data_patch_report(remaining_ops)
    }
}

impl DataPatchRequest {
    fn into_mutation_request(self) -> MutationRequest {
        MutationRequest {
            stop_on_write_error: self.stop_on_write_error,
            ops: self
                .ops
                .into_iter()
                .map(DataPatchOp::into_mutation_op)
                .collect(),
        }
    }

    fn remaining_after_failure(ops: &[DataPatchOp], report: &MutationReport) -> Vec<DataPatchOp> {
        let Some(first_failed) = report.failed.first() else {
            return Vec::new();
        };
        ops.iter()
            .enumerate()
            .filter(|(index, _)| *index >= first_failed.index)
            .map(|(_, op)| op.clone())
            .collect()
    }
}

impl DataPatchOp {
    fn into_mutation_op(self) -> MutationOp {
        match self {
            Self::InsertRecord {
                file,
                sheet,
                actual_type,
                key,
                fields,
                materialization,
            } => MutationOp::InsertRecord {
                file,
                sheet,
                actual_type,
                key,
                fields: MutationFields::Json(fields),
                materialization,
            },
            Self::SetField {
                record,
                file,
                path,
                value,
            } => MutationOp::SetField {
                record: record.into_coordinate(),
                file,
                path,
                value: MutationValue::Json(value),
            },
            Self::RenameRecord {
                record,
                file,
                new_key,
            } => MutationOp::RenameRecord {
                record: record.into_coordinate(),
                file,
                new_key,
            },
            Self::DeleteRecord { record, file } => MutationOp::DeleteRecord {
                record: record.into_coordinate(),
                file,
            },
        }
    }
}

impl PatchRecordSelector {
    fn into_coordinate(self) -> RecordCoordinate {
        RecordCoordinate::new(self.actual_type, self.key)
    }
}

impl MutationReport {
    fn into_data_patch_report(self, remaining_ops: Vec<DataPatchOp>) -> DataPatchReport {
        DataPatchReport {
            write_ok: self.write_ok,
            check_ok: self.check_ok,
            applied: self
                .applied
                .into_iter()
                .map(MutationAppliedOp::into_data_patch_applied)
                .collect(),
            failed: self
                .failed
                .into_iter()
                .map(MutationFailedOp::into_data_patch_failed)
                .collect(),
            affected_files: self.affected_files,
            remaining_ops,
            diagnostics: self.diagnostics,
        }
    }
}

impl MutationAppliedOp {
    fn into_data_patch_applied(self) -> DataPatchAppliedOp {
        DataPatchAppliedOp {
            index: self.index,
            op: self.op,
            record: self.record,
            file: self.file,
            outcome: self.outcome,
        }
    }
}

impl MutationFailedOp {
    fn into_data_patch_failed(self) -> DataPatchFailedOp {
        DataPatchFailedOp {
            index: self.index,
            op: self.op,
            diagnostics: self.diagnostics,
        }
    }
}
