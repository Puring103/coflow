use coflow_api::{Diagnostic, DiagnosticSet, Label, SourceLocation};
use coflow_loader_table_core::writer::TableWriteDiagnostics;
use coflow_loader_table_core::{TableDiagnostic, TableDiagnostics, TableLabel};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LarkDiagnostics {
    pub diagnostics: Vec<LarkDiagnostic>,
}

impl LarkDiagnostics {
    pub(crate) fn one(diagnostic: LarkDiagnostic) -> Self {
        Self {
            diagnostics: vec![diagnostic],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LarkDiagnostic {
    pub code: String,
    pub stage: String,
    pub message: String,
    pub document: Option<String>,
    pub sheet: Option<String>,
}

impl LarkDiagnostic {
    #[must_use]
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            stage: "LARK".to_string(),
            message: message.into(),
            document: None,
            sheet: None,
        }
    }

    #[must_use]
    pub fn with_document(mut self, document: impl Into<String>) -> Self {
        self.document = Some(document.into());
        self
    }

    #[must_use]
    pub fn with_sheet(mut self, sheet: impl Into<String>) -> Self {
        self.sheet = Some(sheet.into());
        self
    }
}

pub(crate) fn lark_diagnostics_to_api(err: LarkDiagnostics) -> DiagnosticSet {
    DiagnosticSet {
        diagnostics: err
            .diagnostics
            .into_iter()
            .map(lark_diagnostic_to_api)
            .collect(),
    }
}

fn lark_diagnostic_to_api(diagnostic: LarkDiagnostic) -> Diagnostic {
    let document = diagnostic.document.clone().unwrap_or_default();
    Diagnostic {
        code: diagnostic.code,
        stage: diagnostic.stage,
        severity: coflow_api::Severity::Error,
        message: diagnostic.message,
        primary: Some(Label {
            location: SourceLocation::RemoteCell {
                document,
                sheet: diagnostic.sheet,
                row: 0,
                column: 0,
            },
            message: None,
        }),
        related: Vec::new(),
    }
}

pub(crate) fn table_diagnostics_to_api(err: TableDiagnostics) -> DiagnosticSet {
    DiagnosticSet {
        diagnostics: err
            .diagnostics
            .into_iter()
            .map(table_diagnostic_to_api)
            .collect(),
    }
}

pub(crate) fn table_write_diagnostics_to_api(err: TableWriteDiagnostics) -> DiagnosticSet {
    err.diagnostics
        .into_iter()
        .map(|diagnostic| diag("LARK-WRITE", diagnostic.message))
        .collect::<Vec<_>>()
        .into()
}

fn table_diagnostic_to_api(diagnostic: TableDiagnostic) -> Diagnostic {
    Diagnostic {
        code: diagnostic.code,
        stage: diagnostic.stage,
        severity: coflow_api::Severity::Error,
        message: diagnostic.message,
        primary: diagnostic.primary.map(table_label_to_api),
        related: diagnostic
            .related
            .into_iter()
            .map(table_label_to_api)
            .collect(),
    }
}

fn table_label_to_api(label: TableLabel) -> Label {
    Label {
        location: coflow_data_model::SourceLocation::from(label.location).into(),
        message: label.message,
    }
}

pub(crate) fn diag(code: &'static str, message: impl Into<String>) -> Diagnostic {
    Diagnostic::error(code, "LARK", message)
}
