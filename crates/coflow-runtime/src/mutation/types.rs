use std::collections::BTreeMap;

use coflow_api::FlatDiagnostic;
use coflow_data_model::{CfdPathSegment, CfdValue};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{RecordCoordinate, WriteOutcome};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MutationRequest {
    #[serde(default = "default_true")]
    pub check_after_write: bool,
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

#[derive(Debug, Clone)]
pub struct PreparedMutation {
    pub(super) stop_on_write_error: bool,
    pub(super) ops: Vec<PreparedMutationOp>,
}

#[derive(Debug, Clone)]
pub(super) enum PreparedMutationOp {
    Pending {
        op: MutationOp,
    },
    InsertRecord {
        file: String,
        sheet: Option<String>,
        actual_type: String,
        key: String,
        fields: BTreeMap<String, CfdValue>,
    },
    SetField {
        record: RecordCoordinate,
        write_file: String,
        path: Vec<coflow_api::WriteFieldPathSegment>,
        value: CfdValue,
    },
    RenameRecord {
        record: RecordCoordinate,
        new_key: String,
        report_file: Option<String>,
    },
    DeleteRecord {
        record: RecordCoordinate,
        report_file: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct MutationReport {
    pub write_ok: bool,
    pub check_ok: bool,
    pub applied: Vec<MutationAppliedOp>,
    pub failed: Vec<MutationFailedOp>,
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
}

const fn default_true() -> bool {
    true
}
