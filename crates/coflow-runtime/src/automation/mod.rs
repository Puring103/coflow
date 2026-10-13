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
pub use execute::{execute,format_source,explain};
pub use transaction::{ChangeSet, FileOperation, Candidate};
pub use operations::{Operation, RecordTarget, FieldPath, InputValue};
pub use request::Arguments;
pub use read::read_command;
pub use snapshot::{InputSnapshot, InputFile};
pub use query::{Selector, Page, query_records};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use crate::{Diagnostic, DiagnosticSet, Severity};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Status { Success, Partial, Invalid, Conflict, Error }

impl Status {
    pub const fn exit_code(self) -> u8 {
        match self { Self::Success => 0, Self::Partial => 1, Self::Invalid => 2, Self::Conflict => 3, Self::Error => 4 }
    }
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct Response {
    pub protocol_version: u32,
    pub command: String,
    pub status: Status,
    pub project: Option<String>,
    pub snapshot: Option<String>,
    pub result: Value,
    #[schemars(with="Vec<Value>")]
    pub diagnostics: Vec<Diagnostic>,
}

impl Response {
    pub fn new(command: impl Into<String>) -> Self {
        Self { protocol_version: 1, command: command.into(), status: Status::Success,
            project: None, snapshot: None, result: Value::Null, diagnostics: Vec::new() }
    }
    pub fn fail(&mut self, failure: Failure) {
        self.status = failure.status;
        self.diagnostics.extend(failure.diagnostics.diagnostics);
    }
}

#[derive(Debug)]
pub struct Failure { pub status: Status, pub diagnostics: DiagnosticSet }
pub type Result<T> = std::result::Result<T, Failure>;

impl Failure {
    pub fn new(status: Status, code: &str, message: impl Into<String>) -> Self {
        Self { status, diagnostics: DiagnosticSet::one(Diagnostic {
            code: code.into(), stage: "CLI".into(), severity: Severity::Error,
            message: message.into(), primary: None, related: Vec::new(), contexts: Vec::new()
        }) }
    }
    pub fn invalid(message: impl Into<String>) -> Self { Self::new(Status::Invalid, "CLI-REQUEST", message) }
    pub fn missing(message: impl Into<String>) -> Self { Self::new(Status::Partial, "CLI-TARGET", message) }
    pub fn conflict(message: impl Into<String>) -> Self { Self::new(Status::Conflict, "CLI-CONFLICT", message) }
}
impl From<DiagnosticSet> for Failure {
    fn from(diagnostics: DiagnosticSet) -> Self { Self { status: Status::Partial, diagnostics } }
}
impl From<std::io::Error> for Failure {
    fn from(error: std::io::Error) -> Self { Self::new(Status::Error, "CLI-IO", error.to_string()) }
}
impl From<serde_json::Error> for Failure {
    fn from(error: serde_json::Error) -> Self { Self::invalid(error.to_string()) }
}

pub(crate) fn json(value: impl Serialize) -> Result<Value> { Ok(serde_json::to_value(value)?) }

pub fn change_set_schema() -> schemars::schema::RootSchema { schemars::schema_for!(ChangeSet) }
pub fn response_schema() -> schemars::schema::RootSchema { schemars::schema_for!(Response) }
