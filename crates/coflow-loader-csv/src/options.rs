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
        .map_err(csv_options_diagnostics)?
        .with_sheet_name(sheet))
}

pub(crate) fn csv_type_for_sheet_from_options(
    options: &Value,
    sheet: Option<&str>,
) -> Result<Option<String>, DiagnosticSet> {
    Ok(csv_table_options_from_options(options)?
        .type_for_sheet(sheet)
        .map_err(csv_options_diagnostics)?
        .map(ToOwned::to_owned))
}

pub(crate) fn csv_sheet_for_type_from_options(
    options: &Value,
    actual_type: &str,
) -> Result<Option<String>, DiagnosticSet> {
    Ok(csv_table_options_from_options(options)?
        .sheet_for_type(actual_type)
        .map_err(csv_options_diagnostics)?
        .map(ToOwned::to_owned))
}

fn csv_table_options_from_options(options: &Value) -> Result<TableSourceOptions, DiagnosticSet> {
    TableSourceOptions::decode(options, "csv source").map_err(csv_options_diagnostics)
}

fn csv_options_diagnostics(err: coflow_loader_table_core::TableOptionsError) -> DiagnosticSet {
    DiagnosticSet::one(Diagnostic::error("CSV-SOURCE", "CSV", err.message))
}
