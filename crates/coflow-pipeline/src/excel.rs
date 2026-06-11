use coflow_cft::CftContainer;
use coflow_loader_excel::{
    load_excel, ExcelDiagnostic, ExcelDiagnostics, ExcelLoadError, ExcelLoadOutput, ExcelLocation,
    ExcelSheet, ExcelSource,
};
use coflow_project::{DiagnosticJson, Project, RelatedJson};
use std::path::Path;

pub(crate) fn load_project_excel(
    project: &Project,
    schema: &CftContainer,
) -> Result<Result<ExcelLoadOutput, Vec<DiagnosticJson>>, String> {
    let sources = excel_sources(project);
    match load_excel(schema, &sources) {
        Ok(output) => {
            if let Some(checks) = &output.check_diagnostics {
                Ok(Err(diagnostics_from_excel_checks(checks)))
            } else {
                Ok(Ok(output))
            }
        }
        Err(err) => Ok(Err(diagnostics_from_excel_error(&err))),
    }
}

fn excel_sources(project: &Project) -> Vec<ExcelSource> {
    project
        .config
        .sources
        .iter()
        .map(|source| {
            let sheets = source
                .sheets
                .iter()
                .map(|sheet| {
                    let mut out = ExcelSheet::new(sheet.sheet.clone());
                    if let Some(type_name) = &sheet.type_name {
                        out = out.with_type(type_name.clone());
                    }
                    if !sheet.columns.is_empty() {
                        out = out.with_columns(sheet.columns.clone());
                    }
                    out
                })
                .collect();
            ExcelSource::new(project.resolve_path(&source.file), sheets)
        })
        .collect()
}

fn excel_diagnostic_json(diagnostic: &ExcelDiagnostic) -> DiagnosticJson {
    let fallback = ExcelLocation::new("");
    let location = diagnostic
        .primary
        .as_ref()
        .map_or(&fallback, |label| &label.location);
    let (line, character) = excel_position(location);
    DiagnosticJson {
        code: diagnostic.source.code.as_str().to_string(),
        stage: diagnostic.source.stage.to_string(),
        severity: "error".to_string(),
        message: diagnostic.source.message.clone(),
        path: location.file.display().to_string(),
        start_line: line,
        start_character: character,
        end_line: line,
        end_character: character.saturating_add(1),
        related: diagnostic
            .related
            .iter()
            .map(|label| excel_related_json(&label.location, label.message.clone()))
            .collect(),
    }
}

fn excel_error_json(
    code: impl Into<String>,
    stage: impl Into<String>,
    message: String,
    file: &Path,
) -> DiagnosticJson {
    DiagnosticJson {
        code: code.into(),
        stage: stage.into(),
        severity: "error".to_string(),
        message,
        path: file.display().to_string(),
        start_line: 0,
        start_character: 0,
        end_line: 0,
        end_character: 1,
        related: Vec::new(),
    }
}

fn excel_location_json(
    code: impl Into<String>,
    stage: impl Into<String>,
    message: String,
    location: &ExcelLocation,
) -> DiagnosticJson {
    let (line, character) = excel_position(location);
    DiagnosticJson {
        code: code.into(),
        stage: stage.into(),
        severity: "error".to_string(),
        message,
        path: location.file.display().to_string(),
        start_line: line,
        start_character: character,
        end_line: line,
        end_character: character.saturating_add(1),
        related: Vec::new(),
    }
}

fn excel_related_json(location: &ExcelLocation, label: Option<String>) -> RelatedJson {
    let (line, character) = excel_position(location);
    RelatedJson {
        path: location.file.display().to_string(),
        start_line: line,
        start_character: character,
        end_line: line,
        end_character: character.saturating_add(1),
        label,
    }
}

fn excel_position(location: &ExcelLocation) -> (usize, usize) {
    (
        location.row.unwrap_or(1).saturating_sub(1),
        location.column.unwrap_or(1).saturating_sub(1),
    )
}

fn diagnostics_from_excel_checks(checks: &ExcelDiagnostics) -> Vec<DiagnosticJson> {
    checks
        .diagnostics
        .iter()
        .map(excel_diagnostic_json)
        .collect()
}

fn diagnostics_from_excel_error(err: &ExcelLoadError) -> Vec<DiagnosticJson> {
    match err {
        ExcelLoadError::OpenWorkbook { file, message } => vec![excel_error_json(
            "EXCEL-OPEN",
            "EXCEL",
            format!("failed to open workbook `{}`: {message}", file.display()),
            file,
        )],
        ExcelLoadError::ReadSheet { location, message } => vec![excel_location_json(
            "EXCEL-SHEET",
            "EXCEL",
            message.clone(),
            location,
        )],
        ExcelLoadError::MissingSheet { file, sheet } => vec![excel_error_json(
            "EXCEL-SHEET",
            "EXCEL",
            format!("workbook `{}` is missing sheet `{sheet}`", file.display()),
            file,
        )],
        ExcelLoadError::EmptySheet { location } => vec![excel_location_json(
            "EXCEL-SHEET",
            "EXCEL",
            "sheet is empty".to_string(),
            location,
        )],
        ExcelLoadError::UnknownType {
            location,
            type_name,
        } => vec![excel_location_json(
            "EXCEL-TYPE",
            "EXCEL",
            format!("unknown CFT type `{type_name}`"),
            location,
        )],
        ExcelLoadError::UnknownColumn {
            location,
            type_name,
            column,
            field,
        } => vec![excel_location_json(
            "EXCEL-COLUMN",
            "EXCEL",
            format!("column `{column}` maps to unknown field `{field}` on type `{type_name}`"),
            location,
        )],
        ExcelLoadError::DuplicateFieldColumn {
            location,
            field,
            first_column,
            duplicate_column,
        } => vec![excel_location_json(
            "EXCEL-COLUMN",
            "EXCEL",
            format!("field `{field}` is mapped by both `{first_column}` and `{duplicate_column}`"),
            location,
        )],
        ExcelLoadError::CellParse {
            location,
            type_name,
            field,
            diagnostics,
        } => diagnostics
            .diagnostics
            .iter()
            .map(|diag| {
                excel_location_json(
                    format!("CELL-{:?}", diag.code),
                    "CELL",
                    format!(
                        "failed to parse `{type_name}.{field}` cell: {}",
                        diag.message
                    ),
                    location,
                )
            })
            .collect(),
        ExcelLoadError::UnsupportedCellValue { location, kind } => vec![excel_location_json(
            "EXCEL-CELL",
            "EXCEL",
            format!("unsupported Excel cell value `{kind}`"),
            location,
        )],
        ExcelLoadError::DataModel(diagnostics) => diagnostics_from_excel_checks(diagnostics),
    }
}
