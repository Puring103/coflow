//! 面向命令行和自动化宿主的公共协议；不依赖桌面编辑器 DTO。

mod snapshot;
mod query;
mod filter;
mod request;
mod read;
mod dimension;
mod references;
mod statistics;
mod rules;
mod context;
mod schema_queries;
mod schema_inspection;
mod transaction;
mod operations;
mod values;
mod rewrite;
mod execute;
mod diagnostics;
pub use diagnostics::CommandDiagnostic;
pub use execute::{execute,format_source,explain};
pub use transaction::{ChangeSet, FileOperation};
pub(crate) use transaction::Candidate;
pub use operations::{Operation, RecordTarget, FieldPath, InputValue};
pub use request::Arguments;
pub use read::read_command;
pub use snapshot::{InputSnapshot, InputFile};
pub use query::{Selector, Page, query_records};

use serde::{Deserialize, Serialize, ser::SerializeStruct};
use serde_json::Value;
use crate::{Diagnostic, DiagnosticSet, Severity};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Status { Success, Partial, Invalid, Conflict, Error }

impl Status {
    pub const fn exit_code(self) -> u8 {
        match self { Self::Success => 0, Self::Partial | Self::Invalid => 1, Self::Conflict => 3, Self::Error => 4 }
    }
}

#[derive(Debug, Clone, Copy, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCategory { Request, Execution }

#[derive(Debug, schemars::JsonSchema)]
pub struct Response {
    pub protocol_version: u32,
    pub command: String,
    pub status: Status,
    pub error_category: Option<ErrorCategory>,
    pub project: Option<String>,
    pub result: Value,
    #[schemars(with="Vec<CommandDiagnostic>")]
    pub diagnostics: Vec<Diagnostic>,
    #[schemars(skip)]
    pub(crate) diagnostic_locations: std::collections::BTreeMap<usize, crate::DiagnosticLogicalLocation>,
    #[schemars(skip)]
    pub(crate) explanations: std::collections::BTreeMap<usize, Value>,
}

impl Serialize for Response {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let mut fields = serializer.serialize_struct("Response", 7)?;
        fields.serialize_field("protocol_version", &self.protocol_version)?;
        fields.serialize_field("command", &self.command)?;
        fields.serialize_field("status", &self.status)?;
        fields.serialize_field("error_category", &self.error_category)?;
        fields.serialize_field("project", &self.project)?;
        fields.serialize_field("result", &self.result)?;
        let root = self.project.as_deref().map(std::path::Path::new);
        let diagnostics: Vec<_> = self.diagnostics.iter().enumerate().map(|(index, diagnostic)|
            CommandDiagnostic::new(diagnostic, self.diagnostic_locations.get(&index), root, self.explanations.get(&index))).collect();
        fields.serialize_field("diagnostics", &diagnostics)?;
        fields.end()
    }
}

impl Response {
    pub fn new(command: impl Into<String>) -> Self {
        Self { protocol_version: 1, command: command.into(), status: Status::Success,
            error_category: None, project: None, result: Value::Null, diagnostics: Vec::new(),
            diagnostic_locations: Default::default(), explanations: Default::default() }
    }
    pub fn fail(&mut self, failure: Failure) {
        self.status = failure.status;
        self.error_category = failure.error_category;
        if let Some(result) = failure.result { self.result = result; }
        self.diagnostics.extend(failure.diagnostics.diagnostics);
    }
    pub fn exit_code(&self) -> u8 {
        if matches!(self.error_category, Some(ErrorCategory::Request)) { 2 } else { self.status.exit_code() }
    }
    pub(crate) fn set_session_diagnostics(&mut self, store: &crate::DiagnosticsStore) {
        self.diagnostics = store.as_set().diagnostics.clone();
        self.diagnostic_locations = (0..self.diagnostics.len()).filter_map(|index| store.logical_location(index).cloned().map(|location| (index, location))).collect();
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
    diagnostics.iter().any(|diagnostic| diagnostic.severity == Severity::Error)
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
        Self { status, error_category: (status == Status::Error).then_some(ErrorCategory::Execution), result: None, diagnostics: DiagnosticSet::one(Diagnostic {
            code: code.into(), stage: "CLI".into(), severity: Severity::Error,
            message: message.into(), primary: None, related: Vec::new(), contexts: Vec::new()
        }) }
    }
    pub fn request(message: impl Into<String>) -> Self {
        let mut failure = Self::new(Status::Error, "CLI-REQUEST", message);
        failure.error_category = Some(ErrorCategory::Request);
        failure
    }
    pub fn missing(message: impl Into<String>) -> Self { Self::new(Status::Invalid, "CLI-TARGET", message) }
    pub fn conflict(message: impl Into<String>) -> Self { Self::new(Status::Conflict, "CLI-CONFLICT", message) }
    pub fn with_result(mut self, result: Value) -> Self { self.result = Some(result); self }
}
impl From<DiagnosticSet> for Failure {
    fn from(diagnostics: DiagnosticSet) -> Self {
        let execution = diagnostics.iter().any(|diagnostic| matches!(diagnostic.code.as_str(),
            "PROJECT-CWD" | "PROJECT-CONFIG-READ" | "PROJECT-CONFIG-PATH" | "CFD-IO" | "CFT-IO")
            || diagnostic.code.starts_with("SELF-UPDATE-") || diagnostic.code.starts_with("CLI-"));
        Self { status: if execution { Status::Error } else { Status::Invalid },
            error_category: execution.then_some(ErrorCategory::Execution), diagnostics, result: None }
    }
}
impl From<std::io::Error> for Failure {
    fn from(error: std::io::Error) -> Self { Self::new(Status::Error, "CLI-IO", error.to_string()) }
}
impl From<serde_json::Error> for Failure {
    fn from(error: serde_json::Error) -> Self { Self::request(error.to_string()) }
}

pub(crate) fn json(value: impl Serialize) -> Result<Value> { Ok(serde_json::to_value(value)?) }

pub fn change_set_schema() -> schemars::schema::RootSchema { schemars::schema_for!(ChangeSet) }
pub fn response_schema() -> schemars::schema::RootSchema { schemars::schema_for!(Response) }
