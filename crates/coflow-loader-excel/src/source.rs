use calamine::{open_workbook_auto, Data, Dimensions, Reader, Sheets};
use coflow_cft::CftSchemaView;
use coflow_data_model::CfdInputRecord;
use coflow_loader_table_core::{
    collect_table_input_records as collect_shared_table_input_records, TableSheet,
    TableSheetConfig, TableSource,
};
use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::diagnostics::{ExcelDiagnostic, ExcelDiagnostics, ExcelLocation};

const DEFAULT_KEY_COLUMN: &str = "id";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExcelSource {
    pub file: PathBuf,
    pub sheets: Vec<ExcelSheet>,
}

impl ExcelSource {
    #[must_use]
    pub fn new(file: impl Into<PathBuf>, sheets: Vec<ExcelSheet>) -> Self {
        Self {
            file: file.into(),
            sheets,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExcelSheet {
    pub sheet: String,
    pub type_name: Option<String>,
    pub key: Option<String>,
    pub columns: BTreeMap<String, String>,
}

impl ExcelSheet {
    #[must_use]
    pub fn new(sheet: impl Into<String>) -> Self {
        Self {
            sheet: sheet.into(),
            type_name: None,
            key: None,
            columns: BTreeMap::new(),
        }
    }

    #[must_use]
    pub fn with_type(mut self, type_name: impl Into<String>) -> Self {
        self.type_name = Some(type_name.into());
        self
    }

    #[must_use]
    pub fn with_key(mut self, key: impl Into<String>) -> Self {
        self.key = Some(key.into());
        self
    }

    #[must_use]
    pub fn with_columns(
        mut self,
        columns: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
    ) -> Self {
        self.columns = columns
            .into_iter()
            .map(|(source, field)| (source.into(), field.into()))
            .collect();
        self
    }

    #[must_use]
    pub fn type_name(&self) -> &str {
        self.type_name.as_deref().map_or(&self.sheet, |name| name)
    }

    #[must_use]
    pub fn key_column(&self) -> &str {
        self.key.as_deref().map_or(DEFAULT_KEY_COLUMN, |key| key)
    }
}

impl From<ExcelSheet> for TableSheetConfig {
    fn from(sheet: ExcelSheet) -> Self {
        let mut out = Self::new(sheet.sheet);
        if let Some(type_name) = sheet.type_name {
            out = out.with_type(type_name);
        }
        if let Some(key) = sheet.key {
            out = out.with_key(key);
        }
        if !sheet.columns.is_empty() {
            out = out.with_columns(sheet.columns);
        }
        out
    }
}

#[derive(Debug, Clone)]
pub struct ExcelInputRecords {
    pub records: Vec<CfdInputRecord>,
}

/// Loads configured Excel sources into input records without building a data model.
///
/// # Errors
///
/// Returns diagnostics when workbooks, sheets, headers, or cells cannot be loaded
/// according to the schema.
pub fn collect_input_records(
    schema: &CftSchemaView,
    sources: &[ExcelSource],
) -> Result<ExcelInputRecords, ExcelDiagnostics> {
    let table_sources = table_sources_from_excel(sources)?;
    collect_shared_table_input_records(schema, &table_sources)
        .map(|loaded| ExcelInputRecords {
            records: loaded.records,
        })
        .map_err(ExcelDiagnostics::from)
}

fn table_sources_from_excel(sources: &[ExcelSource]) -> Result<Vec<TableSource>, ExcelDiagnostics> {
    let mut table_sources = Vec::new();
    let mut diagnostics = Vec::new();
    for source in sources {
        match table_source_from_excel(source) {
            Ok(table_source) => table_sources.push(table_source),
            Err(err) => diagnostics.extend(err.diagnostics),
        }
    }
    if diagnostics.is_empty() {
        Ok(table_sources)
    } else {
        Err(ExcelDiagnostics { diagnostics })
    }
}

fn table_source_from_excel(source: &ExcelSource) -> Result<TableSource, ExcelDiagnostics> {
    let mut diagnostics = Vec::new();
    let mut workbook = match open_workbook_auto(&source.file) {
        Ok(workbook) => workbook,
        Err(err) => {
            diagnostics.push(ExcelDiagnostic::excel(
                "EXCEL-OPEN",
                "EXCEL",
                format!("failed to open workbook `{}`: {err}", source.file.display()),
                ExcelLocation::new(source.file.clone()),
            ));
            return Err(ExcelDiagnostics { diagnostics });
        }
    };

    let sheet_names = workbook.sheet_names();
    let configured_sheets = if source.sheets.is_empty() {
        sheet_names
            .iter()
            .map(|sheet| ExcelSheet::new(sheet.clone()))
            .collect::<Vec<_>>()
    } else {
        source.sheets.clone()
    };

    let mut table_sheets = Vec::new();
    for sheet in &configured_sheets {
        if !sheet_names.iter().any(|name| name == &sheet.sheet) {
            diagnostics.push(ExcelDiagnostic::excel(
                "EXCEL-SHEET",
                "EXCEL",
                format!(
                    "workbook `{}` is missing sheet `{}`",
                    source.file.display(),
                    sheet.sheet
                ),
                ExcelLocation::new(source.file.clone()).sheet(sheet.sheet.clone()),
            ));
            continue;
        }

        let range = match workbook.worksheet_range(&sheet.sheet) {
            Ok(range) => range,
            Err(err) => {
                diagnostics.push(ExcelDiagnostic::excel(
                    "EXCEL-SHEET",
                    "EXCEL",
                    err.to_string(),
                    ExcelLocation::new(source.file.clone()).sheet(sheet.sheet.clone()),
                ));
                continue;
            }
        };
        reject_formula_cells(&mut workbook, source, sheet, &mut diagnostics);
        reject_merged_cells(&mut workbook, source, sheet, &mut diagnostics);

        if range.is_empty() {
            diagnostics.push(ExcelDiagnostic::excel(
                "EXCEL-SHEET",
                "EXCEL",
                "sheet is empty",
                ExcelLocation::new(source.file.clone()).sheet(sheet.sheet.clone()),
            ));
            continue;
        }

        let (range_start_row, range_start_col) = range.start().unwrap_or((0, 0));
        let mut rows = Vec::new();
        for (zero_based_row, row) in range.rows().enumerate() {
            let excel_row = range_start_row as usize + zero_based_row + 1;
            let mut values = Vec::with_capacity(row.len());
            for (zero_based_col, cell) in row.iter().enumerate() {
                let excel_column = range_start_col as usize + zero_based_col + 1;
                let location = ExcelLocation::new(source.file.clone())
                    .sheet(sheet.sheet.clone())
                    .cell(excel_row, excel_column);
                values.push(cell_text(Some(cell), location, &mut diagnostics).unwrap_or_default());
            }
            rows.push(values);
        }
        table_sheets.push(
            TableSheet::new(sheet.sheet.clone(), rows)
                .with_start(range_start_row as usize + 1, range_start_col as usize + 1),
        );
    }

    if diagnostics.is_empty() {
        Ok(TableSource::new(
            source.file.clone(),
            table_sheets,
            configured_sheets
                .into_iter()
                .map(TableSheetConfig::from)
                .collect(),
        ))
    } else {
        Err(ExcelDiagnostics { diagnostics })
    }
}

fn reject_formula_cells(
    workbook: &mut Sheets<std::io::BufReader<std::fs::File>>,
    source: &ExcelSource,
    sheet: &ExcelSheet,
    diagnostics: &mut Vec<ExcelDiagnostic>,
) {
    let Ok(formulas) = workbook.worksheet_formula(&sheet.sheet) else {
        return;
    };
    let (range_start_row, range_start_col) = formulas.start().unwrap_or((0, 0));
    for (zero_based_row, zero_based_col, formula) in formulas.used_cells() {
        if formula.is_empty() {
            continue;
        }
        diagnostics.push(unsupported_cell_diagnostic(
            ExcelLocation::new(source.file.clone()).sheet(sheet.sheet.clone()).cell(
                range_start_row as usize + zero_based_row + 1,
                range_start_col as usize + zero_based_col + 1,
            ),
            "Formula",
        ));
    }
}

fn reject_merged_cells(
    workbook: &mut Sheets<std::io::BufReader<std::fs::File>>,
    source: &ExcelSource,
    sheet: &ExcelSheet,
    diagnostics: &mut Vec<ExcelDiagnostic>,
) {
    let merge_cells = match workbook {
        Sheets::Xls(workbook) => workbook.worksheet_merge_cells(&sheet.sheet).unwrap_or_default(),
        Sheets::Xlsx(workbook) => workbook
            .worksheet_merge_cells(&sheet.sheet)
            .and_then(Result::ok)
            .unwrap_or_default(),
        Sheets::Xlsb(_) | Sheets::Ods(_) => Vec::new(),
    };
    for dimensions in merge_cells {
        diagnostics.push(unsupported_cell_diagnostic(
            ExcelLocation::new(source.file.clone()).sheet(sheet.sheet.clone()).cell(
                dimensions.start.0 as usize + 1,
                dimensions.start.1 as usize + 1,
            ),
            &format!("MergedCell({})", format_dimensions(dimensions)),
        ));
    }
}

fn format_dimensions(dimensions: Dimensions) -> String {
    format!(
        "R{}C{}:R{}C{}",
        dimensions.start.0 + 1,
        dimensions.start.1 + 1,
        dimensions.end.0 + 1,
        dimensions.end.1 + 1
    )
}

fn cell_text(
    cell: Option<&Data>,
    location: ExcelLocation,
    diagnostics: &mut Vec<ExcelDiagnostic>,
) -> Option<String> {
    match cell {
        None | Some(Data::Empty) => Some(String::new()),
        Some(Data::String(value)) => Some(value.clone()),
        Some(Data::Float(value)) if is_whole_float(*value) => Some(format!("{value:.0}")),
        Some(Data::Float(value)) => Some(value.to_string()),
        Some(Data::Int(value)) => Some(value.to_string()),
        Some(Data::Bool(value)) => Some(value.to_string()),
        Some(Data::DateTime(value)) => {
            diagnostics.push(unsupported_cell_diagnostic(
                location,
                &format!("DateTime({value})"),
            ));
            None
        }
        Some(Data::DateTimeIso(value)) => {
            diagnostics.push(unsupported_cell_diagnostic(
                location,
                &format!("DateTimeIso({value})"),
            ));
            None
        }
        Some(Data::DurationIso(value)) => {
            diagnostics.push(unsupported_cell_diagnostic(
                location,
                &format!("DurationIso({value})"),
            ));
            None
        }
        Some(Data::Error(value)) => {
            diagnostics.push(unsupported_cell_diagnostic(
                location,
                &format!("Error({value})"),
            ));
            None
        }
    }
}

fn unsupported_cell_diagnostic(location: ExcelLocation, kind: &str) -> ExcelDiagnostic {
    ExcelDiagnostic::excel(
        "EXCEL-CELL",
        "EXCEL",
        format!("unsupported Excel cell value `{kind}`; store it as text before loading"),
        location,
    )
}

fn is_whole_float(value: f64) -> bool {
    value.is_finite() && value.fract().abs() < f64::EPSILON
}
