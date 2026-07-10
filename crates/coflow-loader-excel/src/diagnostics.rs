use coflow_api::{Diagnostic, DiagnosticSet, Label, Severity, SourceLocation};
use coflow_data_model::{CfdDiagnostic, CfdLabel, CfdRecordId, RecordOrigin};
use coflow_loader_table_core::{
    map_label_to_table, TableDiagnostic, TableDiagnosticKind, TableDiagnostics, TableLabel,
    TableLocation,
};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExcelDiagnostics {
    pub diagnostics: Vec<ExcelDiagnostic>,
}

impl From<TableDiagnostics> for ExcelDiagnostics {
    fn from(diagnostics: TableDiagnostics) -> Self {
        Self {
            diagnostics: diagnostics
                .diagnostics
                .into_iter()
                .map(ExcelDiagnostic::from)
                .collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExcelDiagnostic {
    pub code: String,
    pub stage: String,
    pub message: String,
    pub source: Option<CfdDiagnostic>,
    pub primary: Option<ExcelLabel>,
    pub related: Vec<ExcelLabel>,
}

impl ExcelDiagnostic {
    #[must_use]
    pub fn excel(
        code: impl Into<String>,
        stage: impl Into<String>,
        message: impl Into<String>,
        location: ExcelLocation,
    ) -> Self {
        Self {
            code: code.into(),
            stage: stage.into(),
            message: message.into(),
            source: None,
            primary: Some(ExcelLabel {
                location,
                message: None,
            }),
            related: Vec::new(),
        }
    }
}

impl From<TableDiagnostic> for ExcelDiagnostic {
    fn from(diagnostic: TableDiagnostic) -> Self {
        Self {
            code: diagnostic.provider_code("EXCEL"),
            stage: diagnostic.provider_stage("EXCEL"),
            message: excel_message(&diagnostic),
            source: diagnostic.source,
            primary: diagnostic.primary.map(ExcelLabel::from),
            related: diagnostic
                .related
                .into_iter()
                .map(ExcelLabel::from)
                .collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExcelLabel {
    pub location: ExcelLocation,
    pub message: Option<String>,
}

impl From<TableLabel> for ExcelLabel {
    fn from(label: TableLabel) -> Self {
        Self {
            location: ExcelLocation::from(label.location),
            message: label.message,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExcelLocation {
    pub file: PathBuf,
    pub sheet: Option<String>,
    pub row: Option<usize>,
    pub column: Option<usize>,
}

impl ExcelLocation {
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

    #[must_use]
    pub fn with_row(mut self, row: usize) -> Self {
        self.row = Some(row);
        self
    }

    #[must_use]
    pub fn with_column(mut self, column: Option<usize>) -> Self {
        self.column = column;
        self
    }
}

impl From<TableLocation> for ExcelLocation {
    fn from(location: TableLocation) -> Self {
        Self {
            file: location.file,
            sheet: location.sheet,
            row: location.row,
            column: location.column,
        }
    }
}

/// Map a single CFD label (anchored on a record id) to an `ExcelLabel` using
/// a slice of record origins extracted from input records.
#[must_use]
pub fn map_label_with_record_offset(
    label: &CfdLabel,
    origins: &[RecordOrigin],
    record_offset: usize,
) -> Option<ExcelLabel> {
    let record = label.record?;
    let local_record = record.index().checked_sub(record_offset)?;
    let shifted = label_shifted(label, local_record);
    map_label_to_table(&shifted, origins).map(ExcelLabel::from)
}

fn label_shifted(label: &CfdLabel, new_index: usize) -> CfdLabel {
    CfdLabel {
        record: Some(CfdRecordId::from_index(new_index)),
        path: label.path.clone(),
        message: label.message.clone(),
    }
}

pub(crate) fn excel_diagnostics_to_api(err: ExcelDiagnostics) -> DiagnosticSet {
    DiagnosticSet {
        diagnostics: err
            .diagnostics
            .into_iter()
            .map(excel_diagnostic_to_api)
            .collect(),
    }
}

fn excel_diagnostic_to_api(diagnostic: ExcelDiagnostic) -> Diagnostic {
    Diagnostic {
        code: diagnostic.code,
        stage: diagnostic.stage,
        severity: Severity::Error,
        message: diagnostic.message,
        primary: diagnostic.primary.map(excel_label_to_api),
        related: diagnostic
            .related
            .into_iter()
            .map(excel_label_to_api)
            .collect(),
    }
}

fn excel_label_to_api(label: ExcelLabel) -> Label {
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

fn excel_message(diagnostic: &TableDiagnostic) -> String {
    match diagnostic.kind {
        TableDiagnosticKind::EmptyIdCell => "empty id cell".to_string(),
        _ => diagnostic.message.clone(),
    }
}
