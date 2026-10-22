//! 面向命令行和自动化宿主的公共协议；不依赖桌面编辑器 DTO。

mod files;
mod filter;
mod project_inspection;
mod query;
mod read;
mod request;
mod snapshot;
pub use files::{DataFileRole, DataFileSummary, FileDimensionSummary, ProjectFile, ProjectFiles};
pub use project_inspection::{
    CodegenTarget, ProjectConfiguration, ProjectDimension, ProjectInspection, SchemaInputs,
};
mod dimension;
pub use dimension::{
    DimensionComparison, DimensionField, DimensionGet, DimensionMetadata, DimensionMissing,
    DimensionOrigin, DimensionState, DimensionValue, DimensionVariant, DimensionView,
};
mod references;
pub use references::{
    ReferenceDimension, ReferenceEdge, ReferencePath, ReferencePathSegment, ReferenceQuery,
};
mod statistics;
pub use statistics::ReferenceStatistics;
pub use statistics::{
    DimensionStatistics, FieldStatistics, LengthFrequency, ProjectStatistics, RecordStatistics,
    StatisticMean, StatisticNumber, StatisticQuantile, ValueFrequency,
};
mod rules;
pub use rules::{DependencyScope, RuleDependency, RuleDescription, RuleOwnerKind, RuleQuery};
mod context;
pub use context::{
    ContextDimension, ContextDimensionField, ContextOmission, ContextReference, ContextResult,
    ContextSchema,
};
mod schema_queries;
pub use schema_queries::{SchemaReferences, SchemaUse, SchemaUseKind};
mod execution_contract;
mod inspection_contract;
mod schema_inspection;
pub use execution_contract::{
    CheckResult, CodegenResult, DiffResult, FormatReport, TransactionReport, ValidationReport,
};
pub use inspection_contract::{
    EnumCandidate, InspectedConst, InspectedEnum, InspectedField, InspectedType, OmittedSymbol,
    SchemaInspection, SchemaInspectionResult,
};
pub(crate) mod semantic_diff;
mod transaction;
pub use semantic_diff::{
    DiffFieldState, SchemaDifference, SemanticDifference, SemanticFieldDiff, SemanticRecordDiff,
};
mod model_value;
mod operations;
mod typed_references;
mod values;
pub use model_value::{DictionaryEntry, ModelValue};
mod diagnostics;
mod execute;
mod explanations;
mod presentation;
pub use presentation::DiagnosticsSummary;
mod rewrite;
pub use diagnostics::CommandDiagnostic;
pub use execute::{execute, explain, format_source};
pub use explanations::DiagnosticExplanation;
pub use operations::{FieldPath, InputValue, Operation, RecordFields, RecordTarget};
pub use query::{FieldSource, RecordResult};
pub use query::{Page, Selector};
pub use read::{read_command, SchemaSymbol, SourceRead};
pub use read::{DraftField, DraftResult, RecordDraft};
pub use request::Arguments;
pub use snapshot::{InputFile, InputSnapshot};
pub(crate) use transaction::publication_failure;
pub(crate) use transaction::Candidate;
pub use transaction::{ChangeSet, FileOperation};

use crate::{Diagnostic, DiagnosticSet, Severity};
use serde::{ser::SerializeStruct, Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Success,
    Partial,
    Invalid,
    Conflict,
    Error,
}

impl Status {
    pub const fn exit_code(self) -> u8 {
        match self {
            Self::Success => 0,
            Self::Partial | Self::Invalid => 1,
            Self::Conflict => 3,
            Self::Error => 4,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCategory {
    Request,
    Execution,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct IncompleteDataFile {
    pub file: String,
    pub reasons: Vec<String>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct RejectedDataRecord {
    pub record: RecordTarget,
    pub file: String,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct DataScope {
    pub complete: bool,
    pub available_records: usize,
    pub selected_file_count: usize,
    pub incomplete_file_count: usize,
    pub rejected_record_count: usize,
    pub details_omitted: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub selected_files: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub incomplete_files: Vec<IncompleteDataFile>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rejected_records: Vec<RejectedDataRecord>,
}

#[derive(Debug, schemars::JsonSchema)]
pub struct Response<T = Value> {
    #[schemars(range(min = 1, max = 1))]
    pub protocol_version: u32,
    pub command: String,
    pub status: Status,
    pub error_category: Option<ErrorCategory>,
    pub project: Option<String>,
    pub revision: Option<String>,
    pub result: T,
    pub data_scope: Option<DataScope>,
    pub diagnostics_summary: Option<DiagnosticsSummary>,
    #[schemars(with = "Vec<CommandDiagnostic>")]
    pub diagnostics: Vec<Diagnostic>,
    #[schemars(skip)]
    pub(crate) diagnostic_locations:
        std::collections::BTreeMap<usize, crate::DiagnosticLogicalLocation>,
    #[schemars(skip)]
    pub(crate) explanations: std::collections::BTreeMap<usize, Value>,
    #[schemars(skip)]
    diagnostic_help: std::collections::BTreeMap<usize, String>,
}

impl<T: Serialize> Serialize for Response<T> {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        let mut fields = serializer.serialize_struct("Response", 10)?;
        fields.serialize_field("protocol_version", &self.protocol_version)?;
        fields.serialize_field("command", &self.command)?;
        fields.serialize_field("status", &self.status)?;
        fields.serialize_field("error_category", &self.error_category)?;
        fields.serialize_field("project", &self.project)?;
        fields.serialize_field("revision", &self.revision)?;
        fields.serialize_field("result", &self.result)?;
        fields.serialize_field("data_scope", &self.data_scope)?;
        fields.serialize_field("diagnostics_summary", &self.diagnostics_summary)?;
        let root = self.project.as_deref().map(std::path::Path::new);
        let diagnostics: Vec<_> = self
            .diagnostics
            .iter()
            .enumerate()
            .map(|(index, diagnostic)| {
                let mut output = CommandDiagnostic::new(
                    diagnostic,
                    self.diagnostic_locations.get(&index),
                    root,
                    self.explanations.get(&index),
                );
                if let Some(help) = self.diagnostic_help.get(&index) {
                    output.help = Some(help.clone());
                } else if diagnostic.code == "CLI-REQUEST" {
                    output.help = Some(format!(
                        "查看参数和请求格式：coflow describe {} --json",
                        self.command
                    ));
                }
                output
            })
            .collect();
        fields.serialize_field("diagnostics", &diagnostics)?;
        fields.end()
    }
}

impl Response {
    pub fn new(command: impl Into<String>) -> Self {
        Self {
            protocol_version: 1,
            command: command.into(),
            status: Status::Success,
            error_category: None,
            project: None,
            revision: None,
            result: Value::Null,
            data_scope: None,
            diagnostics_summary: None,
            diagnostics: Vec::new(),
            diagnostic_locations: Default::default(),
            explanations: Default::default(),
            diagnostic_help: Default::default(),
        }
    }
    pub fn set_diagnostic_help(&mut self, index: usize, help: String) {
        self.diagnostic_help.insert(index, help);
    }
    pub fn fail(&mut self, failure: Failure) {
        self.status = failure.status;
        self.error_category = failure.error_category;
        if let Some(result) = failure.result {
            if let Some(revision) = result.get("revision") {
                self.revision = revision.as_str().map(str::to_owned);
            }
            self.result = result;
        }
        self.diagnostics.extend(failure.diagnostics.diagnostics);
    }
    pub fn exit_code(&self) -> u8 {
        if matches!(self.error_category, Some(ErrorCategory::Request)) {
            2
        } else {
            self.status.exit_code()
        }
    }
    pub(crate) fn set_session_diagnostics(&mut self, store: &crate::DiagnosticsStore) {
        self.diagnostics = store.as_set().diagnostics.clone();
        self.diagnostic_locations = (0..self.diagnostics.len())
            .filter_map(|index| {
                store
                    .logical_location(index)
                    .cloned()
                    .map(|location| (index, location))
            })
            .collect();
    }
    pub(crate) fn set_data_scope(
        &mut self,
        session: &crate::ReadOnlyProjectSession,
        snapshot: &InputSnapshot,
    ) {
        let root = session.session.project.root_dir();
        let mut incomplete = std::collections::BTreeMap::<String, Vec<String>>::new();
        for diagnostic in session
            .queries()
            .diagnostics()
            .as_set()
            .iter()
            .filter(|diagnostic| diagnostic.severity == Severity::Error)
        {
            if let Some(label) = &diagnostic.primary {
                let location =
                    diagnostics::CommandLocation::from_source(&label.location, Some(root));
                if location.path.ends_with(".cfd") {
                    incomplete
                        .entry(location.path)
                        .or_default()
                        .push(diagnostic.message.clone());
                }
            }
        }
        // 拒绝记录保留身份与来源；计数范围不随列表筛选或分页改变。
        let rejected_records: Vec<_> = session
            .queries()
            .rejected_records()
            .iter()
            .map(|record| RejectedDataRecord {
                record: RecordTarget {
                    actual_type: record.actual_type.clone(),
                    id: record.id.clone(),
                },
                file: record.display_path.clone(),
            })
            .collect();
        let selected_files: Vec<_> = snapshot
            .files
            .iter()
            .filter(|file| file.kind == "data")
            .map(|file| file.path.clone())
            .collect();
        self.data_scope = Some(DataScope {
            complete: !session
                .queries()
                .diagnostics()
                .as_set()
                .iter()
                .any(|diagnostic| diagnostic.severity == Severity::Error),
            available_records: session.model().records().count(),
            selected_file_count: selected_files.len(),
            incomplete_file_count: incomplete.len(),
            rejected_record_count: rejected_records.len(),
            details_omitted: false,
            selected_files,
            incomplete_files: incomplete
                .into_iter()
                .map(|(file, reasons)| IncompleteDataFile { file, reasons })
                .collect(),
            rejected_records,
        });
    }
    pub(crate) fn set_validation(&mut self, validation: rules::Validation) -> Value {
        self.status = validation.status();
        self.diagnostics = validation.diagnostics.diagnostics;
        self.diagnostic_locations = validation.logical_locations;
        self.explanations = validation.explanations;
        validation.report
    }
}

pub(crate) fn has_errors(diagnostics: &DiagnosticSet) -> bool {
    diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == Severity::Error)
}

#[derive(Debug)]
pub struct Failure {
    pub status: Status,
    pub error_category: Option<ErrorCategory>,
    pub diagnostics: DiagnosticSet,
    pub result: Option<Value>,
}
pub type Result<T> = std::result::Result<T, Failure>;

impl Failure {
    pub fn new(status: Status, code: &str, message: impl Into<String>) -> Self {
        Self {
            status,
            error_category: (status == Status::Error).then_some(ErrorCategory::Execution),
            result: None,
            diagnostics: DiagnosticSet::one(Diagnostic {
                code: code.into(),
                stage: "CLI".into(),
                severity: Severity::Error,
                message: message.into(),
                primary: None,
                related: Vec::new(),
                contexts: Vec::new(),
            }),
        }
    }
    pub fn request(message: impl Into<String>) -> Self {
        let mut failure = Self::new(Status::Error, "CLI-REQUEST", message);
        failure.error_category = Some(ErrorCategory::Request);
        failure
    }
    pub fn missing(message: impl Into<String>) -> Self {
        Self::new(Status::Invalid, "CLI-TARGET", message)
    }
    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(Status::Conflict, "CLI-CONFLICT", message)
    }
    pub fn with_result(mut self, result: Value) -> Self {
        self.result = Some(result);
        self
    }
}
impl From<DiagnosticSet> for Failure {
    fn from(diagnostics: DiagnosticSet) -> Self {
        let request = diagnostics.iter().any(|diagnostic| {
            matches!(
                diagnostic.code.as_str(),
                "CLI-VALUE" | "CLI-PATH" | "CLI-FIELD" | "CLI-SORT" | "CLI-FILTER"
            )
        });
        let execution = diagnostics.iter().any(|diagnostic| {
            matches!(
                diagnostic.code.as_str(),
                "PROJECT-CWD"
                    | "PROJECT-CONFIG-READ"
                    | "PROJECT-CONFIG-PATH"
                    | "CFD-READ"
                    | "CFD-IO"
                    | "CFT-IO"
                    | "ARTIFACT-IO"
                    | "GIT-DIFF"
            ) || diagnostic.code.starts_with("SELF-UPDATE-")
                || diagnostic.code.starts_with("SKILL-")
                || diagnostic.code.starts_with("CLI-")
        });
        let conflict = diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "WRITE-CONFLICT");
        Self {
            status: if conflict {
                Status::Conflict
            } else if execution {
                Status::Error
            } else {
                Status::Invalid
            },
            error_category: if request {
                Some(ErrorCategory::Request)
            } else {
                execution.then_some(ErrorCategory::Execution)
            },
            diagnostics,
            result: None,
        }
    }
}
impl From<std::io::Error> for Failure {
    fn from(error: std::io::Error) -> Self {
        Self::new(Status::Error, "CLI-IO", error.to_string())
    }
}
impl From<serde_json::Error> for Failure {
    fn from(error: serde_json::Error) -> Self {
        Self::request(error.to_string())
    }
}

pub(crate) fn json(value: impl Serialize) -> Result<Value> {
    Ok(serde_json::to_value(value)?)
}

pub fn change_set_schema() -> schemars::schema::RootSchema {
    constrain_schema(schemars::schema_for!(ChangeSet))
}
pub fn record_fields_schema() -> schemars::schema::RootSchema {
    constrain_schema(schemars::schema_for!(RecordFields))
}
pub fn response_schema() -> schemars::schema::RootSchema {
    response_schema_of::<Value>()
}
pub fn response_schema_for(command: Option<&str>) -> schemars::schema::RootSchema {
    match command {
        Some("check" | "schema.check") => response_schema_of::<Option<CheckResult>>(),
        Some("diff") => response_schema_of::<Option<DiffResult>>(),
        Some("format") => response_schema_of::<Option<FormatReport>>(),
        Some("codegen" | "build") => response_schema_of::<Option<CodegenResult>>(),
        Some(
            "apply"
            | "source.write"
            | "source.create"
            | "source.delete"
            | "project.add-input"
            | "project.remove-input"
            | "data.create"
            | "data.set"
            | "data.unset"
            | "data.rename"
            | "data.delete"
            | "data.transfer"
            | "dimension.set"
            | "dimension.clear",
        ) => response_schema_of::<Option<TransactionReport>>(),
        Some("context") => response_schema_of::<Option<ContextResult>>(),
        Some("explain") => response_schema_of::<Option<DiagnosticExplanation>>(),
        Some("data.refs") => response_schema_of::<Option<ReferenceQuery>>(),
        Some("source.read") => response_schema_of::<Option<SourceRead>>(),
        Some("project.files") => response_schema_of::<Option<ProjectFiles>>(),
        Some("project.inspect") => response_schema_of::<Option<ProjectInspection>>(),
        Some("dimension.list") => response_schema_of::<Option<Vec<DimensionMetadata>>>(),
        Some("dimension.inspect") => response_schema_of::<Option<DimensionMetadata>>(),
        Some("dimension.variants") => response_schema_of::<Option<Page<DimensionVariant>>>(),
        Some("dimension.get") => response_schema_of::<Option<DimensionGet>>(),
        Some("dimension.missing") => response_schema_of::<Option<Page<DimensionMissing>>>(),
        Some("dimension.diff") => response_schema_of::<Option<Page<DimensionComparison>>>(),
        Some("stats.project") => response_schema_of::<Option<ProjectStatistics>>(),
        Some("stats.records") => response_schema_of::<Option<Page<RecordStatistics>>>(),
        Some("stats.dimension") => response_schema_of::<Option<Page<DimensionStatistics>>>(),
        Some("stats.field") => response_schema_of::<Option<FieldStatistics>>(),
        Some("stats.refs") => response_schema_of::<Option<Page<ReferenceStatistics>>>(),
        Some("rule.list" | "rule.inspect") => response_schema_of::<Option<RuleQuery>>(),
        Some("schema.symbols") => response_schema_of::<Option<Page<SchemaSymbol>>>(),
        Some("schema.refs") => response_schema_of::<Option<SchemaReferences>>(),
        Some("schema.inspect") => response_schema_of::<Option<SchemaInspectionResult>>(),
        Some("data.get") => response_schema_of::<Option<RecordResult>>(),
        Some("data.draft") => response_schema_of::<Option<DraftResult>>(),
        Some("data.list" | "data.search") => response_schema_of::<Option<Page<RecordResult>>>(),
        _ => response_schema(),
    }
}

#[derive(schemars::JsonSchema)]
#[serde(untagged)]
#[allow(dead_code)]
enum ResponsePayload<T> {
    Result(T),
    Failure(execution_contract::FailureContext),
}

pub fn response_schema_of<T: schemars::JsonSchema>() -> schemars::schema::RootSchema {
    // 请求或前置条件失败可能发生在候选事务创建前，Schema 同样描述这些实际输出。
    let mut schema = schemars::schema_for!(Response<ResponsePayload<T>>);
    // 信封始终输出全部属性；Option 的空值是 null，而不是省略该属性。
    if let Some(object) = schema.schema.object.as_mut() {
        object.required.extend(object.properties.keys().cloned());
    }
    constrain_schema(schema)
}

/// 补充 DTO 的跨字段语义约束；输入与输出 Schema 使用同一个定义遍历。
fn constrain_schema(mut root: schemars::schema::RootSchema) -> schemars::schema::RootSchema {
    use schemars::schema::{Schema, SchemaObject, SubschemaValidation};
    fn constraint(value: Value) -> Schema {
        serde_json::from_value(value).expect("static schema constraint")
    }
    fn visit(schema: &mut SchemaObject) {
        if let Some(object) = schema.object.as_mut() {
            let tag = |name: &str| {
                object.properties.get(name).and_then(|schema| match schema {
                    Schema::Object(schema) => schema.enum_values.as_ref()?.first()?.as_str(),
                    _ => None,
                })
            };
            let restriction = if tag("kind") == Some("enum") {
                Some(SubschemaValidation {
                    any_of: Some(vec![
                        constraint(
                            serde_json::json!({"required":["value"],"properties":{"value":{"type":"string"}}}),
                        ),
                        constraint(
                            serde_json::json!({"required":["variant"],"properties":{"variant":{"type":"string"}}}),
                        ),
                    ]),
                    ..Default::default()
                })
            } else if matches!(tag("op"), Some("write" | "create"))
                && object.properties.contains_key("content_file")
            {
                Some(SubschemaValidation {
                    one_of: Some(vec![
                        constraint(
                            serde_json::json!({"required":["content"],"properties":{"content":{"type":"string"},"content_file":{"type":"null"}}}),
                        ),
                        constraint(
                            serde_json::json!({"required":["content_file"],"properties":{"content_file":{"type":"string"},"content":{"type":"null"}}}),
                        ),
                    ]),
                    ..Default::default()
                })
            } else {
                None
            };
            if let Some(restriction) = restriction {
                schema.subschemas = Some(Box::new(restriction));
            }
            for child in object.properties.values_mut() {
                if let Schema::Object(child) = child {
                    visit(child);
                }
            }
        }
        if let Some(subschemas) = schema.subschemas.as_mut() {
            for children in [
                &mut subschemas.one_of,
                &mut subschemas.any_of,
                &mut subschemas.all_of,
            ]
            .into_iter()
            .flatten()
            {
                for child in children {
                    if let Schema::Object(child) = child {
                        visit(child);
                    }
                }
            }
        }
    }
    visit(&mut root.schema);
    for schema in root.definitions.values_mut() {
        if let Schema::Object(schema) = schema {
            visit(schema);
        }
    }
    root
}
