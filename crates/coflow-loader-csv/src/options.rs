use crate::CsvSheet;
use coflow_api::{Diagnostic, DiagnosticSet};
use coflow_loader_table_core::{TableSheetConfig, TableSourceOptions};
use serde_json::Value;

pub(super) fn csv_sheets_from_options(options: &Value) -> Result<Vec<CsvSheet>, DiagnosticSet> {
    Ok(csv_table_options_from_options(options)?
        .into_sheets()
        .into_iter()
        .map(CsvSheet::from)
        .collect())
}

pub(crate) fn csv_sheet_config_from_options(
    options: &Value,
    sheet: &str,
    actual_type: &str,
) -> Result<TableSheetConfig, DiagnosticSet> {
    Ok(csv_table_options_from_options(options)?
        .sheet_config(sheet, actual_type)
        .with_sheet_name(sheet))
}

fn csv_table_options_from_options(options: &Value) -> Result<TableSourceOptions, DiagnosticSet> {
    TableSourceOptions::decode(options, "csv source").map_err(|err| {
        DiagnosticSet::one(Diagnostic::error("CSV-SOURCE", "CSV", err.message))
    })
}
