//! Excel `.xlsx` loader for Coflow data models.
//!
//! This crate deliberately accepts already-parsed loader configuration. YAML,
//! JSON, editor settings, and command-line parsing should live in higher layers.

#![cfg_attr(
    not(test),
    deny(
        clippy::dbg_macro,
        clippy::expect_used,
        clippy::panic,
        clippy::panic_in_result_fn,
        clippy::todo,
        clippy::unimplemented,
        clippy::unreachable,
        clippy::unwrap_used
    )
)]
#![allow(clippy::missing_const_for_fn, clippy::multiple_crate_versions)]

use calamine::{open_workbook_auto, Data, Reader};
use coflow_api::{
    DataLoader, Diagnostic, DiagnosticSet, LoadContext, LoadedRecords, LoaderDescriptor,
    ProbeResult, ProjectSourceRef, ResolvedSource, SourceLocationSpec, SourceResolveContext,
};
use coflow_cft::CftContainer;
use coflow_data_model::CfdInputRecord;
use coflow_loader_table_core::{
    collect_table_input_records as collect_shared_table_input_records, TableSheet,
    TableSheetConfig, TableSource,
};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

mod diagnostics;
mod options;
pub mod writer;
use diagnostics::excel_diagnostics_to_api;
pub use diagnostics::{
    map_label_with_record_offset, ExcelDiagnostic, ExcelDiagnostics, ExcelLabel, ExcelLocation,
};
use options::excel_sheets_from_options;
pub use writer::{ExcelWriter, EXCEL_WRITER_DESCRIPTOR};

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
    schema: &CftContainer,
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

#[derive(Debug, Default, Clone, Copy)]
pub struct ExcelLoader;

pub const EXCEL_LOADER_DESCRIPTOR: LoaderDescriptor = LoaderDescriptor {
    id: "excel",
    display_name: "Excel workbook",
    extensions: &["xlsx", "xlsm", "xls"],
    uri_schemes: &[],
    option_keys: &["sheets"],
};

impl DataLoader for ExcelLoader {
    fn descriptor(&self) -> &'static LoaderDescriptor {
        &EXCEL_LOADER_DESCRIPTOR
    }

    fn probe(&self, source: &ProjectSourceRef<'_>) -> ProbeResult {
        if source.source_type == Some(EXCEL_LOADER_DESCRIPTOR.id) {
            return ProbeResult::certain();
        }
        if matches!(
            source.location,
            SourceLocationSpec::Path(path)
                if path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| EXCEL_LOADER_DESCRIPTOR.extensions.contains(&ext))
        ) {
            ProbeResult::likely()
        } else {
            ProbeResult::none()
        }
    }

    fn resolve(
        &self,
        _ctx: SourceResolveContext<'_>,
        source: &ResolvedSource,
    ) -> Result<Vec<ResolvedSource>, DiagnosticSet> {
        let SourceLocationSpec::Path(path) = &source.location else {
            if source.provider_id == EXCEL_LOADER_DESCRIPTOR.id {
                return Err(DiagnosticSet::one(Diagnostic::error(
                    "EXCEL-SOURCE",
                    "EXCEL",
                    "excel source requires `path`",
                )));
            }
            return Ok(Vec::new());
        };
        if path.is_dir() {
            return collect_excel_sources(path, source);
        }
        if is_excel_path(path) {
            return Ok(vec![source.clone()]);
        }
        Err(DiagnosticSet::one(Diagnostic::error(
            "EXCEL-SOURCE",
            "EXCEL",
            format!(
                "source file `{}` has unsupported extension",
                source.display_name
            ),
        )))
    }

    fn load(
        &self,
        ctx: LoadContext<'_>,
        source: &ResolvedSource,
    ) -> Result<LoadedRecords, DiagnosticSet> {
        let SourceLocationSpec::Path(file) = &source.location else {
            return Err(DiagnosticSet::one(Diagnostic::error(
                "EXCEL-SOURCE",
                "EXCEL",
                "excel source requires `path`",
            )));
        };
        let sheets = excel_sheets_from_options(&source.options)?;
        let excel_source = ExcelSource::new(file.clone(), sheets);
        collect_input_records(ctx.schema, &[excel_source])
            .map(|loaded| LoadedRecords {
                records: loaded.records,
            })
            .map_err(excel_diagnostics_to_api)
    }
}

fn collect_excel_sources(
    dir: &Path,
    source: &ResolvedSource,
) -> Result<Vec<ResolvedSource>, DiagnosticSet> {
    let mut entries = fs::read_dir(dir)
        .map_err(|err| {
            DiagnosticSet::one(Diagnostic::error(
                "EXCEL-SOURCE",
                "EXCEL",
                format!("failed to read data source dir `{}`: {err}", dir.display()),
            ))
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| {
            DiagnosticSet::one(Diagnostic::error(
                "EXCEL-SOURCE",
                "EXCEL",
                format!("failed to read data source dir `{}`: {err}", dir.display()),
            ))
        })?;
    entries.sort_by_key(fs::DirEntry::path);

    let mut sources = Vec::new();
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            sources.extend(collect_excel_sources(&path, source)?);
        } else if is_excel_path(&path) {
            sources.push(ResolvedSource {
                provider_id: EXCEL_LOADER_DESCRIPTOR.id.to_string(),
                display_name: path.display().to_string(),
                location: SourceLocationSpec::Path(path),
                options: source.options.clone(),
            });
        }
    }
    Ok(sources)
}

fn is_excel_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| EXCEL_LOADER_DESCRIPTOR.extensions.contains(&ext))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::panic)]

    use super::*;
    use serde_json::json;
    use std::path::Path;

    #[test]
    fn rejects_empty_sheet_name_in_options() {
        let Err(err) = excel_sheets_from_options(&json!({
            "sheets": [
                {
                    "sheet": "",
                    "columns": {
                        "A": "id"
                    }
                }
            ]
        })) else {
            panic!("empty sheet should fail");
        };

        assert!(err
            .iter()
            .any(|diagnostic| diagnostic.message == "excel source sheet `sheet` is empty"));
    }

    #[test]
    fn explicit_excel_loader_rejects_url_source() {
        let loader = ExcelLoader;
        let schema = CftContainer::new();
        let source = ResolvedSource {
            provider_id: EXCEL_LOADER_DESCRIPTOR.id.to_string(),
            location: SourceLocationSpec::Uri("https://example.test/configs.xlsx".to_string()),
            options: json!({}),
            display_name: "https://example.test/configs.xlsx".to_string(),
        };

        let Err(err) = loader.resolve(
            SourceResolveContext {
                project_root: Path::new("."),
                schema: &schema,
            },
            &source,
        ) else {
            panic!("excel url source should fail");
        };

        assert!(err
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("excel source requires `path`")));
    }
}
