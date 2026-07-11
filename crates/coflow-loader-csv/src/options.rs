use crate::{CsvSheet, CSV_LOADER_DESCRIPTOR};
use coflow_api::{
    DecodedSourceOptions, Diagnostic, DiagnosticSet, Label, ResolvedSource, SourceLocation,
};
use coflow_loader_table_core::{TableSheetConfig, TableSourceOptions};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CsvSourceOptions {
    table: TableSourceOptions,
}

pub(crate) fn decode_csv_source_options(
    raw: &Value,
) -> Result<DecodedSourceOptions, DiagnosticSet> {
    validate_option_keys(raw, &["sheets"])?;
    let table = TableSourceOptions::decode(raw, "csv source").map_err(csv_options_diagnostics)?;
    Ok(DecodedSourceOptions::new(
        CSV_LOADER_DESCRIPTOR.id,
        CsvSourceOptions { table },
    ))
}

pub(crate) fn csv_source_options(
    source: &ResolvedSource,
) -> Result<&CsvSourceOptions, DiagnosticSet> {
    source.options(CSV_LOADER_DESCRIPTOR.id)
}

pub(super) fn csv_sheets(options: &CsvSourceOptions) -> Vec<CsvSheet> {
    options
        .table
        .clone()
        .into_sheets()
        .into_iter()
        .map(CsvSheet::from)
        .collect()
}

pub(crate) fn csv_sheet_config_from_options(
    options: &CsvSourceOptions,
    sheet: &str,
    actual_type: &str,
) -> Result<TableSheetConfig, DiagnosticSet> {
    Ok(options
        .table
        .sheet_config(sheet, actual_type)
        .map_err(csv_options_diagnostics)?
        .with_sheet_name(sheet))
}

pub(crate) fn csv_type_for_sheet_from_options(
    options: &CsvSourceOptions,
    sheet: Option<&str>,
) -> Result<Option<String>, DiagnosticSet> {
    Ok(options
        .table
        .type_for_sheet(sheet)
        .map_err(csv_options_diagnostics)?
        .map(ToOwned::to_owned))
}

pub(crate) fn csv_sheet_for_type_from_options(
    options: &CsvSourceOptions,
    actual_type: &str,
) -> Result<Option<String>, DiagnosticSet> {
    Ok(options
        .table
        .sheet_for_type(actual_type)
        .map_err(csv_options_diagnostics)?
        .map(ToOwned::to_owned))
}

fn csv_options_diagnostics(err: coflow_loader_table_core::TableOptionsError) -> DiagnosticSet {
    option_error(["sheets"], err.message)
}

fn validate_option_keys(raw: &Value, allowed: &[&str]) -> Result<(), DiagnosticSet> {
    let Some(options) = raw.as_object() else {
        if raw.is_null() {
            return Ok(());
        }
        return Err(option_error([], "csv source options must be an object"));
    };
    if let Some(key) = options.keys().find(|key| !allowed.contains(&key.as_str())) {
        return Err(option_error(
            [key.as_str()],
            format!("unknown csv source option `{key}`"),
        ));
    }
    Ok(())
}

fn option_error<'a>(
    key_path: impl IntoIterator<Item = &'a str>,
    message: impl Into<String>,
) -> DiagnosticSet {
    DiagnosticSet::one(
        Diagnostic::error("CSV-SOURCE", "CSV", message).with_primary(Label {
            location: SourceLocation::ProjectConfig {
                path: std::path::PathBuf::new(),
                key_path: key_path.into_iter().map(str::to_string).collect(),
            },
            message: None,
        }),
    )
}
