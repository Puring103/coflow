use std::collections::BTreeMap;

use coflow_api::FlatDiagnostic;
use coflow_cft::{DimensionName, FieldName, RecordKey, TypeName, VariantName};
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
    SetDimensionValue {
        coordinate: PatchDimensionValueSelector,
        #[serde(default)]
        expected: crate::DimensionValueExpectation,
        value: Value,
    },
    ClearDimensionValue {
        coordinate: PatchDimensionValueSelector,
        #[serde(default)]
        expected: crate::DimensionValueExpectation,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatchDimensionValueSelector {
    pub record: PatchRecordSelector,
    pub field: FieldName,
    pub dimension: DimensionName,
    pub variant: VariantName,
    #[serde(default)]
    pub path: Vec<CfdPathSegment>,
}

impl PatchDimensionValueSelector {
    fn into_coordinate(self) -> Result<crate::DimensionValueCoordinate, coflow_cft::CftNameError> {
        Ok(crate::DimensionValueCoordinate {
            actual_type: TypeName::new(self.record.actual_type)?,
            record_key: RecordKey::new(self.record.key)?,
            field: self.field,
            dimension: self.dimension,
            variant: self.variant,
            path: self.path,
        })
    }
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
        let mutation_request = match request.into_mutation_request() {
            Ok(request) => request,
            Err((index, op, diagnostics)) => {
                return DataPatchReport {
                    write_ok: false,
                    check_ok: false,
                    applied: Vec::new(),
                    failed: vec![DataPatchFailedOp {
                        index,
                        op,
                        diagnostics: diagnostics.flat_diagnostics(),
                    }],
                    affected_files: Vec::new(),
                    remaining_ops: original_ops,
                    diagnostics: Vec::new(),
                };
            }
        };
        let mutation_report = self.apply_mutation(mutation_request);
        let remaining_ops =
            DataPatchRequest::remaining_after_failure(&original_ops, &mutation_report);
        mutation_report.into_data_patch_report(remaining_ops)
    }
}

impl DataPatchRequest {
    fn into_mutation_request(
        self,
    ) -> Result<MutationRequest, (usize, String, coflow_api::DiagnosticSet)> {
        let mut ops = Vec::with_capacity(self.ops.len());
        for (index, op) in self.ops.into_iter().enumerate() {
            let op_name = op.name().to_string();
            let mutation = op.into_mutation_op().map_err(|error| {
                (
                    index,
                    op_name,
                    coflow_api::DiagnosticSet::one(coflow_api::Diagnostic::error(
                        "PATCH-DIMENSION-COORDINATE",
                        "PATCH",
                        error.to_string(),
                    )),
                )
            })?;
            ops.push(mutation);
        }
        Ok(MutationRequest {
            stop_on_write_error: self.stop_on_write_error,
            ops,
        })
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
    const fn name(&self) -> &'static str {
        match self {
            Self::InsertRecord { .. } => "insert_record",
            Self::SetField { .. } => "set_field",
            Self::SetDimensionValue { .. } => "set_dimension_value",
            Self::ClearDimensionValue { .. } => "clear_dimension_value",
            Self::RenameRecord { .. } => "rename_record",
            Self::DeleteRecord { .. } => "delete_record",
        }
    }

    fn into_mutation_op(self) -> Result<MutationOp, coflow_cft::CftNameError> {
        Ok(match self {
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
            Self::SetDimensionValue {
                coordinate,
                expected,
                value,
            } => MutationOp::SetDimensionValue {
                coordinate: coordinate.into_coordinate()?,
                expected,
                value: MutationValue::Json(value),
            },
            Self::ClearDimensionValue {
                coordinate,
                expected,
            } => MutationOp::ClearDimensionValue {
                coordinate: coordinate.into_coordinate()?,
                expected,
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
        })
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
