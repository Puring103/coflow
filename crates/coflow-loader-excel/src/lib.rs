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
pub use coflow_api::table::TableSheet;
use coflow_api::table::{
    collect_table_input_records as collect_shared_table_input_records, map_label_to_table,
    map_table_diagnostics, TableDiagnostic, TableDiagnostics, TableLabel, TableLocation,
    TableSheetConfig, TableSource as SharedTableSource,
};
use coflow_api::{
    origins_of, DataLoader, Diagnostic, DiagnosticSet, Label, LoadContext, LoadedRecords,
    LoaderDescriptor, ProbeResult, ProjectSourceRef, RecordOrigin, ResolvedSource, SourceLocation,
    SourceLocationSpec, SourceResolveContext,
};
use coflow_cft::CftContainer;
use coflow_data_model::{CfdDataModel, CfdDiagnostic, CfdInputRecord};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub mod writer;
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

#[derive(Debug, Clone, PartialEq)]
pub struct ExcelLoadOutput {
    pub model: CfdDataModel,
    pub check_diagnostics: Option<ExcelDiagnostics>,
}

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

#[derive(Debug, Clone)]
pub struct ExcelInputRecords {
    pub records: Vec<CfdInputRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableSource {
    pub name: PathBuf,
    pub sheets: Vec<TableSheet>,
    pub configs: Vec<ExcelSheet>,
}

impl TableSource {
    #[must_use]
    pub fn new(
        name: impl Into<PathBuf>,
        sheets: Vec<TableSheet>,
        configs: Vec<ExcelSheet>,
    ) -> Self {
        Self {
            name: name.into(),
            sheets,
            configs,
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
            code: table_code_to_excel(&diagnostic.code),
            stage: table_stage_to_excel(&diagnostic.stage),
            message: table_message_to_excel(&diagnostic.message),
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
    label: &coflow_data_model::CfdLabel,
    origins: &[RecordOrigin],
    record_offset: usize,
) -> Option<ExcelLabel> {
    let record = label.record?;
    let local_record = record.index().checked_sub(record_offset)?;
    let shifted = label_shifted(label, local_record);
    map_label_to_table(&shifted, origins).map(ExcelLabel::from)
}

fn label_shifted(
    label: &coflow_data_model::CfdLabel,
    new_index: usize,
) -> coflow_data_model::CfdLabel {
    coflow_data_model::CfdLabel {
        record: Some(coflow_data_model::CfdRecordId::from_index(new_index)),
        path: label.path.clone(),
        message: label.message.clone(),
    }
}

/// Loads configured Excel sheets into a validated data model without running
/// CFT `check` blocks.
///
/// The caller is responsible for parsing any YAML/JSON/CLI configuration and
/// compiling the provided schema container before calling this function.
///
/// # Errors
///
/// Returns Excel-stage errors, cell syntax errors, or data-model diagnostics.
pub fn load_excel_model(
    schema: &CftContainer,
    sources: &[ExcelSource],
) -> Result<CfdDataModel, ExcelDiagnostics> {
    let table_sources = table_sources_from_excel(sources)?;
    let loaded = collect_shared_table_input_records(schema, &table_sources)
        .map_err(ExcelDiagnostics::from)?;
    let origins = origins_of(&loaded.records);
    let mut builder = CfdDataModel::builder(schema);
    for record in loaded.records {
        builder.add_input_record(record);
    }
    builder
        .build()
        .map_err(|diagnostics| ExcelDiagnostics::from(map_table_diagnostics(diagnostics, &origins)))
}

/// Loads configured Excel sheets and runs CFT `check` blocks against the model.
///
/// Check diagnostics are returned alongside the model because check failures do
/// not invalidate the constructed data model.
///
/// # Errors
///
/// Returns Excel-stage errors, cell syntax errors, or data-model diagnostics.
pub fn load_excel(
    schema: &CftContainer,
    sources: &[ExcelSource],
) -> Result<ExcelLoadOutput, ExcelDiagnostics> {
    let table_sources = table_sources_from_excel(sources)?;
    let loaded = collect_shared_table_input_records(schema, &table_sources)
        .map_err(ExcelDiagnostics::from)?;
    let origins = origins_of(&loaded.records);
    let mut builder = CfdDataModel::builder(schema);
    for record in loaded.records {
        builder.add_input_record(record);
    }
    let model = builder.build().map_err(|diagnostics| {
        ExcelDiagnostics::from(map_table_diagnostics(diagnostics, &origins))
    })?;
    let check_diagnostics = coflow_checker::run_checks(schema, &model)
        .err()
        .map(|diagnostics| ExcelDiagnostics::from(map_table_diagnostics(diagnostics, &origins)));
    Ok(ExcelLoadOutput {
        model,
        check_diagnostics,
    })
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

/// Loads already-read table sources into input records without building a data model.
///
/// This is the shared Excel-like path used by local workbooks and remote sheet
/// providers. Source readers own I/O and convert cells to strings before
/// calling this function.
///
/// # Errors
///
/// Returns diagnostics when sheets, headers, or cells cannot be loaded
/// according to the schema.
pub fn collect_table_input_records(
    schema: &CftContainer,
    sources: &[TableSource],
) -> Result<ExcelInputRecords, ExcelDiagnostics> {
    let shared_sources = sources
        .iter()
        .cloned()
        .map(shared_table_source_from_excel_table_source)
        .collect::<Vec<_>>();
    collect_shared_table_input_records(schema, &shared_sources)
        .map(|loaded| ExcelInputRecords {
            records: loaded.records,
        })
        .map_err(ExcelDiagnostics::from)
}

fn table_sources_from_excel(
    sources: &[ExcelSource],
) -> Result<Vec<SharedTableSource>, ExcelDiagnostics> {
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

fn table_source_from_excel(source: &ExcelSource) -> Result<SharedTableSource, ExcelDiagnostics> {
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
        Ok(SharedTableSource::new(
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

fn shared_table_source_from_excel_table_source(source: TableSource) -> SharedTableSource {
    SharedTableSource::new(
        source.name,
        source.sheets,
        source
            .configs
            .into_iter()
            .map(TableSheetConfig::from)
            .collect(),
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

fn table_code_to_excel(code: &str) -> String {
    code.strip_prefix("TABLE-").map_or_else(
        || code.to_string(),
        |suffix| match suffix {
            "TYPE" => "EXCEL-TYPE".to_string(),
            "ID" => "EXCEL-ID".to_string(),
            "SHEET" => "EXCEL-SHEET".to_string(),
            "COLUMN" => "EXCEL-COLUMN".to_string(),
            other => format!("EXCEL-{other}"),
        },
    )
}

fn table_stage_to_excel(stage: &str) -> String {
    if stage == "TABLE" {
        "EXCEL".to_string()
    } else {
        stage.to_string()
    }
}

fn table_message_to_excel(message: &str) -> String {
    if message == "record key cell is empty" {
        "empty id cell".to_string()
    } else {
        message.to_string()
    }
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

fn excel_sheets_from_options(options: &Value) -> Result<Vec<ExcelSheet>, DiagnosticSet> {
    let Some(sheets) = options.get("sheets") else {
        return Ok(Vec::new());
    };
    let Some(sheets) = sheets.as_array() else {
        return Err(DiagnosticSet::one(Diagnostic::error(
            "EXCEL-SOURCE",
            "EXCEL",
            "excel source option `sheets` must be an array",
        )));
    };
    sheets
        .iter()
        .map(excel_sheet_from_value)
        .collect::<Result<Vec<_>, _>>()
}

fn excel_sheet_from_value(value: &Value) -> Result<ExcelSheet, DiagnosticSet> {
    let Some(object) = value.as_object() else {
        return Err(DiagnosticSet::one(Diagnostic::error(
            "EXCEL-SOURCE",
            "EXCEL",
            "excel source sheet config must be an object",
        )));
    };
    let Some(sheet_name) = object.get("sheet").and_then(Value::as_str) else {
        return Err(DiagnosticSet::one(Diagnostic::error(
            "EXCEL-SOURCE",
            "EXCEL",
            "excel source sheet config requires `sheet`",
        )));
    };
    if sheet_name.trim().is_empty() {
        return Err(DiagnosticSet::one(Diagnostic::error(
            "EXCEL-SOURCE",
            "EXCEL",
            "excel source sheet `sheet` is empty",
        )));
    }
    let mut sheet = ExcelSheet::new(sheet_name);
    if let Some(type_name) = optional_string_field(object, "type", "excel source sheet `type`")? {
        if type_name.trim().is_empty() {
            return Err(DiagnosticSet::one(Diagnostic::error(
                "EXCEL-SOURCE",
                "EXCEL",
                "excel source sheet `type` is empty",
            )));
        }
        sheet = sheet.with_type(type_name);
    }
    if let Some(key) = optional_string_field(object, "key", "excel source sheet `key`")? {
        if key.trim().is_empty() {
            return Err(DiagnosticSet::one(Diagnostic::error(
                "EXCEL-SOURCE",
                "EXCEL",
                "excel source sheet `key` is empty",
            )));
        }
        sheet = sheet.with_key(key);
    }
    if let Some(columns) = object.get("columns") {
        let Some(columns) = columns.as_object() else {
            return Err(DiagnosticSet::one(Diagnostic::error(
                "EXCEL-SOURCE",
                "EXCEL",
                "excel source sheet `columns` must be an object",
            )));
        };
        let mut parsed_columns = Vec::new();
        for (source, field) in columns {
            let Some(field) = field.as_str() else {
                return Err(DiagnosticSet::one(Diagnostic::error(
                    "EXCEL-SOURCE",
                    "EXCEL",
                    format!("excel source sheet column `{source}` must map to a string field"),
                )));
            };
            if source.trim().is_empty() {
                return Err(DiagnosticSet::one(Diagnostic::error(
                    "EXCEL-SOURCE",
                    "EXCEL",
                    "excel source sheet column name is empty",
                )));
            }
            if field.trim().is_empty() {
                return Err(DiagnosticSet::one(Diagnostic::error(
                    "EXCEL-SOURCE",
                    "EXCEL",
                    format!("excel source sheet column `{source}` maps to an empty field"),
                )));
            }
            parsed_columns.push((source.as_str(), field));
        }
        sheet = sheet.with_columns(parsed_columns);
    }
    Ok(sheet)
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

fn optional_string_field<'a>(
    object: &'a serde_json::Map<String, Value>,
    key: &str,
    label: &str,
) -> Result<Option<&'a str>, DiagnosticSet> {
    let Some(value) = object.get(key) else {
        return Ok(None);
    };
    value.as_str().map(Some).ok_or_else(|| {
        DiagnosticSet::one(Diagnostic::error(
            "EXCEL-SOURCE",
            "EXCEL",
            format!("{label} must be a string"),
        ))
    })
}

fn excel_diagnostics_to_api(err: ExcelDiagnostics) -> DiagnosticSet {
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
        severity: coflow_api::Severity::Error,
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
