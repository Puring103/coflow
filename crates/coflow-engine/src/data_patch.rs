use std::collections::BTreeMap;

use coflow_api::{DiagnosticSet, FlatDiagnostic, ProviderRegistry};
use coflow_data_model::CfdPathSegment;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    DefaultMaterialization, MutationAppliedOp, MutationFailedOp, MutationFields, MutationOp,
    MutationReport, MutationRequest, MutationValue, ProjectSession, RecordCoordinate, WriteOutcome,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataPatchRequest {
    #[serde(default = "default_true")]
    pub check_after_write: bool,
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

impl ProjectSession {
    /// Apply a JSON data patch through the engine mutation API.
    ///
    /// This keeps the CLI-facing data patch DTOs as a thin adapter while all
    /// mutation semantics live in the shared engine path.
    ///
    /// # Errors
    ///
    /// Returns diagnostics only when mutation execution cannot produce a
    /// report. Per-operation validation and writer failures are represented in
    /// the returned [`DataPatchReport`].
    pub fn apply_data_patch(
        &mut self,
        registry: &ProviderRegistry,
        request: DataPatchRequest,
    ) -> Result<DataPatchReport, DiagnosticSet> {
        let mutation_report = self.apply_mutation(registry, request.into_mutation_request())?;
        Ok(mutation_report.into_data_patch_report())
    }
}

impl DataPatchRequest {
    fn into_mutation_request(self) -> MutationRequest {
        MutationRequest {
            check_after_write: self.check_after_write,
            stop_on_write_error: self.stop_on_write_error,
            ops: self
                .ops
                .into_iter()
                .map(DataPatchOp::into_mutation_op)
                .collect(),
        }
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
    fn into_data_patch_report(self) -> DataPatchReport {
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
