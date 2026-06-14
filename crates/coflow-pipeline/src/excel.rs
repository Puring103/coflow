use coflow_cft::CftContainer;
use coflow_loader_excel::{
    load_excel, ExcelDiagnostic, ExcelDiagnostics, ExcelLoadOutput, ExcelLocation, ExcelSheet,
    ExcelSource,
};
use coflow_project::{DiagnosticJson, Project, RelatedJson};

pub fn load_project_excel(
    project: &Project,
    schema: &CftContainer,
) -> Result<ExcelLoadOutput, Vec<DiagnosticJson>> {
    let sources = excel_sources(project);
    match load_excel(schema, &sources) {
        Ok(output) => output
            .check_diagnostics
            .clone()
            .map_or(Ok(output), |checks| {
                Err(diagnostics_from_excel_checks(&checks))
            }),
        Err(err) => Err(diagnostics_from_excel_checks(&err)),
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
        code: diagnostic.code.clone(),
        stage: diagnostic.stage.clone(),
        severity: "error".to_string(),
        message: diagnostic.message.clone(),
        path: location.file.display().to_string(),
        sheet: location.sheet.clone(),
        cell: excel_cell(location),
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

fn excel_related_json(location: &ExcelLocation, label: Option<String>) -> RelatedJson {
    let (line, character) = excel_position(location);
    RelatedJson {
        path: location.file.display().to_string(),
        sheet: location.sheet.clone(),
        cell: excel_cell(location),
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

fn excel_cell(location: &ExcelLocation) -> Option<String> {
    Some(format!(
        "{}{}",
        excel_column_name(location.column?),
        location.row?
    ))
}

fn excel_column_name(column: usize) -> String {
    let mut value = column;
    let mut name = Vec::new();
    while value > 0 {
        value -= 1;
        name.push((b'A' + (value % 26) as u8) as char);
        value /= 26;
    }
    name.iter().rev().collect()
}

fn diagnostics_from_excel_checks(checks: &ExcelDiagnostics) -> Vec<DiagnosticJson> {
    checks
        .diagnostics
        .iter()
        .map(excel_diagnostic_json)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use coflow_data_model::{CfdDiagnostic, CfdErrorCode};
    use std::path::Path;

    #[test]
    fn excel_cell_omits_partial_coordinates_and_handles_multi_letter_columns() {
        let file = Path::new("data.xlsx");
        assert_eq!(excel_cell(&ExcelLocation::new(file).with_row(3)), None);
        assert_eq!(
            excel_cell(&ExcelLocation::new(file).with_column(Some(27))),
            None
        );
        assert_eq!(
            excel_cell(&ExcelLocation::new(file).cell(12, 28)),
            Some("AB12".to_string())
        );
    }

    #[test]
    fn excel_position_saturates_zero_and_defaults_missing_coordinates() {
        assert_eq!(excel_position(&ExcelLocation::new("data.xlsx")), (0, 0));
        assert_eq!(
            excel_position(&ExcelLocation::new("data.xlsx").cell(0, 0)),
            (0, 0)
        );
    }

    #[test]
    fn excel_diagnostic_without_primary_uses_empty_fallback_location() {
        let diagnostic = ExcelDiagnostic {
            code: CfdErrorCode::CheckFailed.as_str().to_string(),
            stage: "CHECK".to_string(),
            message: "bad check".to_string(),
            source: Some(CfdDiagnostic::error(CfdErrorCode::CheckFailed, "bad check")),
            primary: None,
            related: Vec::new(),
        };

        let json = excel_diagnostic_json(&diagnostic);

        assert_eq!(json.path, "");
        assert_eq!(json.cell, None);
        assert_eq!(json.start_line, 0);
        assert_eq!(json.end_character, 1);
    }

    #[test]
    fn excel_related_json_preserves_optional_sheet_cell_and_label() {
        let related = excel_related_json(
            &ExcelLocation::new("data.xlsx").sheet("Items").cell(2, 52),
            Some("duplicate here".to_string()),
        );

        assert_eq!(related.path, "data.xlsx");
        assert_eq!(related.sheet.as_deref(), Some("Items"));
        assert_eq!(related.cell.as_deref(), Some("AZ2"));
        assert_eq!(related.start_line, 1);
        assert_eq!(related.start_character, 51);
        assert_eq!(related.label.as_deref(), Some("duplicate here"));
    }
}
