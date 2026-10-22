//! 执行命令使用与查询命令一致的具体结果契约，自描述不退化成任意 JSON。
use super::{ModelValue, RecordTarget, SchemaDifference, SemanticFieldDiff, SemanticRecordDiff};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum RuleScope {
    All(String),
    Selected(Vec<String>),
}
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct RuleExecution {
    pub rule: Option<String>,
    pub statement: Option<usize>,
    pub diagnostic_count: Option<usize>,
    pub state: String,
    pub target: Value,
}
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ValidationReport {
    pub schema: Option<String>,
    pub structure: String,
    pub rules: String,
    pub complete: bool,
    pub diagnostic_count: Option<usize>,
    pub scope: Option<RuleScope>,
    pub tasks: Option<Vec<RuleExecution>>,
    pub results: Option<Vec<RuleExecution>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule_counts: Option<std::collections::BTreeMap<String, usize>>,
    pub requested_tasks: Option<usize>,
    pub executed_tasks: Option<usize>,
    pub rejected_tasks: Option<usize>,
    pub performance: Option<Value>,
}
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum OperationDescription {
    File(super::FileOperation),
    Data(super::Operation),
}
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct OperationOutcome {
    pub index: usize,
    pub status: String,
    pub operation: OperationDescription,
    pub details: Option<Value>,
    pub reason: Option<Vec<String>>,
}
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct SourceDifference {
    pub path: String,
    pub change: Option<String>,
    pub patch: Option<String>,
    pub before: Option<String>,
    pub after: Option<String>,
}
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct MaterializedField {
    pub record: RecordTarget,
    pub field: String,
    pub origin: String,
    pub location: Option<super::diagnostics::CommandLocation>,
    pub value: ModelValue,
}
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct AutomaticUpdate {
    pub kind: String,
    pub file: String,
    pub record: Option<RecordTarget>,
    pub new_id: Option<String>,
    pub count: Option<usize>,
}
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct RestorationFailure {
    pub path: String,
    pub reason: String,
    pub conflict: bool,
}
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ValuePrecondition {
    pub view: String,
    pub expected: ModelValue,
    pub actual: Option<ModelValue>,
}
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum IdentityPrecondition {
    Revision { expected: String, actual: String },
    CursorRevision { expected: String, actual: String },
    Fingerprint { expected: String, actual: String },
    Count { expected: usize, actual: usize },
}
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Precondition {
    Value(ValuePrecondition),
    Identity(IdentityPrecondition),
}
/// 请求尚未形成候选事务时返回的定位与前置条件，不要求伪造一份事务结果。
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FailureContext {
    pub written: Option<bool>,
    pub revision: Option<String>,
    pub precondition: Option<Precondition>,
    pub request_path: Option<String>,
    pub operation_index: Option<usize>,
    pub target: Option<Value>,
    pub line: Option<usize>,
    pub column: Option<usize>,
}
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct DiffSelection {
    pub semantic_available: bool,
    pub schema_available: bool,
    pub changed_records: usize,
    pub changed_fields: usize,
    pub changed_dimension_fields: usize,
    pub changed_schema_symbols: usize,
    pub schema: Option<Vec<SchemaDifference>>,
    pub records: Option<Vec<SemanticRecordDiff>>,
    pub fields: Option<Vec<SemanticFieldDiff>>,
    pub dimensions: Option<Vec<SemanticFieldDiff>>,
}
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct TransactionReport {
    pub changed: Option<bool>,
    pub written: bool,
    pub dry_run: bool,
    pub validation: ValidationReport,
    pub operations: Vec<OperationOutcome>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation_counts: Option<std::collections::BTreeMap<String, usize>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision_before: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision_after: Option<String>,
    pub files: Option<Vec<SourceDifference>>,
    pub written_files: Vec<String>,
    pub matched_records: Option<usize>,
    pub changed_records: Option<usize>,
    pub changed_fields: Option<usize>,
    pub materialized: Option<Vec<MaterializedField>>,
    pub automatic_updates: Option<Vec<AutomaticUpdate>>,
    pub diff: Option<DiffSelection>,
    pub endpoints: Option<DiffEndpoints>,
    pub restored_files: Option<Vec<String>>,
    pub restoration_failed: Option<Vec<RestorationFailure>>,
    pub failed_operation: Option<usize>,
    pub precondition: Option<Precondition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation_index: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
}
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct MissingInput {
    pub path: String,
    pub reason: String,
}
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct DiffEndpoint {
    pub config: String,
    pub complete: bool,
    pub missing_inputs: Option<Vec<MissingInput>>,
}
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct DiffEndpoints {
    pub base: DiffEndpoint,
    pub current: DiffEndpoint,
}
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct DiffDiagnostic {
    pub endpoint: String,
    pub code: String,
    pub message: String,
}
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct GitDiffReport {
    #[serde(flatten)]
    pub difference: DiffSelection,
    pub base: Option<String>,
    pub comparison: Option<String>,
    pub validation: Option<ValidationReport>,
    pub endpoints: DiffEndpoints,
    pub files: Option<Vec<SourceDifference>>,
    pub diagnostics: Vec<DiffDiagnostic>,
}
#[derive(Debug, JsonSchema)]
#[serde(untagged)]
pub enum DiffResult {
    Git(GitDiffReport),
    Candidate(TransactionReport),
}
#[derive(Debug, JsonSchema)]
#[serde(untagged)]
pub enum CheckResult {
    Current(ValidationReport),
    Candidate(TransactionReport),
}
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct FormatReport {
    pub changed: bool,
    pub written: bool,
    pub dry_run: bool,
    pub check: Option<bool>,
    pub source: Option<String>,
    pub files: Option<Vec<SourceDifference>>,
    pub written_files: Option<Vec<String>>,
    pub restored_files: Option<Vec<String>>,
    pub restoration_failed: Option<Vec<RestorationFailure>>,
    pub revision: Option<String>,
    pub precondition: Option<Precondition>,
}
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ArtifactDifference {
    pub path: String,
    pub change: String,
    pub mapping: Option<Value>,
}
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ArtifactReport {
    pub changed: bool,
    pub files: Vec<ArtifactDifference>,
    pub written: bool,
    pub dry_run: bool,
    pub written_files: Vec<String>,
    pub restored_files: Option<Vec<String>>,
    pub restoration_failed: Option<Vec<RestorationFailure>>,
}
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct CodegenResult {
    pub validation: Option<ValidationReport>,
    pub artifacts: Option<ArtifactReport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub written: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub precondition: Option<Precondition>,
}
