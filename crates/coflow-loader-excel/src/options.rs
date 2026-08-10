use crate::{ExcelSheet, EXCEL_LOADER_DESCRIPTOR};
use coflow_api::{
    DecodedSourceOptions, Diagnostic, DiagnosticSet, Label, ResolvedSource, SourceLocation,
};
use coflow_loader_table_core::{TableSheetConfig, TableSourceOptions};
use serde_json::Value;

pub(crate) type ExcelSourceOptions = TableSourceOptions;

pub(crate) fn decode_excel_source_options(
    raw: &Value,
) -> Result<DecodedSourceOptions, DiagnosticSet> {
    let table =
        TableSourceOptions::decode(raw, "excel source").map_err(excel_options_diagnostics)?;
    Ok(DecodedSourceOptions::new(
        EXCEL_LOADER_DESCRIPTOR.id,
        table,
    ))
}

pub(crate) fn excel_source_options(
    source: &ResolvedSource,
) -> Result<&ExcelSourceOptions, DiagnosticSet> {
    source.options(EXCEL_LOADER_DESCRIPTOR.id)
}

pub(super) fn excel_sheets(options: &ExcelSourceOptions) -> Vec<ExcelSheet> {
    options
        .clone()
        .into_sheets()
        .into_iter()
        .map(ExcelSheet::from)
        .collect()
}

pub(crate) fn excel_sheet_config_from_options(
    options: &ExcelSourceOptions,
    sheet: &str,
    actual_type: &str,
) -> Result<TableSheetConfig, DiagnosticSet> {
    Ok(options
        .sheet_config(sheet, actual_type)
        .map_err(excel_options_diagnostics)?
        .with_sheet_name(sheet))
}

pub(crate) fn excel_sheet_for_type_from_options(
    options: &ExcelSourceOptions,
    actual_type: &str,
) -> Result<Option<String>, DiagnosticSet> {
    Ok(options
        .sheet_for_type(actual_type)
        .map_err(excel_options_diagnostics)?
        .map(ToOwned::to_owned))
}

pub(crate) fn excel_type_for_sheet_from_options(
    options: &ExcelSourceOptions,
    sheet: Option<&str>,
) -> Result<Option<String>, DiagnosticSet> {
    Ok(options
        .type_for_sheet(sheet)
        .map_err(excel_options_diagnostics)?
        .map(ToOwned::to_owned))
}

fn excel_options_diagnostics(err: coflow_loader_table_core::TableOptionsError) -> DiagnosticSet {
    option_error(err.key_path.iter().map(String::as_str), err.message)
}

fn option_error<'a>(
    key_path: impl IntoIterator<Item = &'a str>,
    message: impl Into<String>,
) -> DiagnosticSet {
    DiagnosticSet::one(
        Diagnostic::error("EXCEL-SOURCE", "EXCEL", message).with_primary(Label {
            location: SourceLocation::ProjectConfig {
                path: std::path::PathBuf::new(),
                key_path: key_path.into_iter().map(str::to_string).collect(),
            },
            message: None,
        }),
    )
}
