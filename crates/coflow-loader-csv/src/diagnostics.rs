use coflow_api::{Diagnostic, DiagnosticSet, Label, Severity, SourceLocation};
use coflow_data_model::CfdDiagnostic;
use coflow_loader_table_core::{TableDiagnostic, TableDiagnostics, TableLabel, TableLocation};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsvDiagnostics {
    pub diagnostics: Vec<CsvDiagnostic>,
}

impl From<TableDiagnostics> for CsvDiagnostics {
    fn from(diagnostics: TableDiagnostics) -> Self {
        Self {
            diagnostics: diagnostics
                .diagnostics
                .into_iter()
                .map(CsvDiagnostic::from)
                .collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsvDiagnostic {
    pub code: String,
    pub stage: String,
    pub message: String,
    pub source: Option<CfdDiagnostic>,
    pub primary: Option<CsvLabel>,
    pub related: Vec<CsvLabel>,
}

impl CsvDiagnostic {
    #[must_use]
    pub fn csv(
        code: impl Into<String>,
        stage: impl Into<String>,
        message: impl Into<String>,
        location: CsvLocation,
    ) -> Self {
        Self {
            code: code.into(),
            stage: stage.into(),
            message: message.into(),
            source: None,
            primary: Some(CsvLabel {
                location,
                message: None,
            }),
            related: Vec::new(),
        }
    }
}

impl From<TableDiagnostic> for CsvDiagnostic {
    fn from(diagnostic: TableDiagnostic) -> Self {
        Self {
            code: table_code_to_csv(&diagnostic.code),
            stage: table_stage_to_csv(&diagnostic.stage),
            message: diagnostic.message,
            source: diagnostic.source,
            primary: diagnostic.primary.map(CsvLabel::from),
            related: diagnostic.related.into_iter().map(CsvLabel::from).collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsvLabel {
    pub location: CsvLocation,
    pub message: Option<String>,
}

impl From<TableLabel> for CsvLabel {
    fn from(label: TableLabel) -> Self {
        Self {
            location: CsvLocation::from(label.location),
            message: label.message,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsvLocation {
    pub file: PathBuf,
    pub sheet: Option<String>,
    pub row: Option<usize>,
    pub column: Option<usize>,
}

impl CsvLocation {
    #[must_use]
    pub fn new(file: impl Into<PathBuf>) -> Self {
        Self {
            file: file.into(),
            sheet: None,
            row: None,
            column: None,
        }
    }

    #[must_use]
    pub fn sheet(mut self, sheet: impl Into<String>) -> Self {
        self.sheet = Some(sheet.into());
        self
    }

    #[must_use]
    pub fn cell(mut self, row: usize, column: usize) -> Self {
        self.row = Some(row);
        self.column = Some(column);
        self
    }
}

impl From<TableLocation> for CsvLocation {
    fn from(location: TableLocation) -> Self {
        Self {
            file: location.file,
            sheet: location.sheet,
            row: location.row,
            column: location.column,
        }
    }
}

pub fn csv_diagnostics_to_api(err: CsvDiagnostics) -> DiagnosticSet {
    DiagnosticSet {
        diagnostics: err
            .diagnostics
            .into_iter()
            .map(csv_diagnostic_to_api)
            .collect(),
    }
}

fn csv_diagnostic_to_api(diagnostic: CsvDiagnostic) -> Diagnostic {
    Diagnostic {
        code: diagnostic.code,
        stage: diagnostic.stage,
        severity: Severity::Error,
        message: diagnostic.message,
        primary: diagnostic.primary.map(csv_label_to_api),
        related: diagnostic
            .related
            .into_iter()
            .map(csv_label_to_api)
            .collect(),
    }
}

fn csv_label_to_api(label: CsvLabel) -> Label {
    Label {
        location: SourceLocation::TableCell {
            path: label.location.file,
            sheet: label.location.sheet,
            row: label.location.row.unwrap_or(1),
            column: label.location.column.unwrap_or(1),
        },
        message: label.message,
    }
}

fn table_code_to_csv(code: &str) -> String {
    code.strip_prefix("TABLE-").map_or_else(
        || code.to_string(),
        |suffix| match suffix {
            "TYPE" => "CSV-TYPE".to_string(),
            "ID" => "CSV-ID".to_string(),
            "SHEET" => "CSV-SHEET".to_string(),
            "COLUMN" => "CSV-COLUMN".to_string(),
            other => format!("CSV-{other}"),
        },
    )
}

fn table_stage_to_csv(stage: &str) -> String {
    if stage == "TABLE" {
        "CSV".to_string()
    } else {
        stage.to_string()
    }
}
