//! CSV loader for Coflow data models.
//!
//! This crate owns the shared RFC 4180 parser/writer used by both the data
//! loader and localization CSV tables.

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
#![allow(clippy::missing_const_for_fn)]

mod format;
mod options;
pub mod writer;
pub use format::{parse, write};
pub use writer::CsvWriter;

use coflow_api::{
    DataLoader, Diagnostic, DiagnosticSet, Label, LoadContext, LoadedRecords, LoaderDescriptor,
    ProbeResult, ProjectSourceRef, ResolvedSource, Severity, SourceLocation, SourceLocationSpec,
    SourceResolveContext,
};
use coflow_cft::CftContainer;
use coflow_data_model::{CfdDiagnostic, CfdInputRecord};
use coflow_loader_table_core::{
    collect_table_input_records as collect_shared_table_input_records, TableDiagnostic,
    TableDiagnostics, TableLabel, TableLocation, TableSheet, TableSheetConfig, TableSource,
};
use options::csv_sheets_from_options;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsvSource {
    pub file: PathBuf,
    pub sheets: Vec<CsvSheet>,
}

impl CsvSource {
    #[must_use]
    pub fn new(file: impl Into<PathBuf>, sheets: Vec<CsvSheet>) -> Self {
        Self {
            file: file.into(),
            sheets,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsvSheet {
    pub sheet: String,
    pub type_name: Option<String>,
    pub key: Option<String>,
    pub columns: BTreeMap<String, String>,
}

impl CsvSheet {
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
}

impl From<CsvSheet> for TableSheetConfig {
    fn from(sheet: CsvSheet) -> Self {
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsvDiagnostics {
    pub diagnostics: Vec<CsvDiagnostic>,
}

impl From<TableDiagnostics> for CsvDiagnostics {
    fn from(diagnostics: TableDiagnostics) -> Self {
        Self {
            diagnostics: diagnostics
                .diagnostics
                .into_iter()
                .map(CsvDiagnostic::from)
                .collect(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CsvInputRecords {
    pub records: Vec<CfdInputRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsvDiagnostic {
    pub code: String,
    pub stage: String,
    pub message: String,
    pub source: Option<CfdDiagnostic>,
    pub primary: Option<CsvLabel>,
    pub related: Vec<CsvLabel>,
}

impl CsvDiagnostic {
    #[must_use]
    pub fn csv(
        code: impl Into<String>,
        stage: impl Into<String>,
        message: impl Into<String>,
        location: CsvLocation,
    ) -> Self {
        Self {
            code: code.into(),
            stage: stage.into(),
            message: message.into(),
            source: None,
            primary: Some(CsvLabel {
                location,
                message: None,
            }),
            related: Vec::new(),
        }
    }
}

impl From<TableDiagnostic> for CsvDiagnostic {
    fn from(diagnostic: TableDiagnostic) -> Self {
        Self {
            code: table_code_to_csv(&diagnostic.code),
            stage: table_stage_to_csv(&diagnostic.stage),
            message: diagnostic.message,
            source: diagnostic.source,
            primary: diagnostic.primary.map(CsvLabel::from),
            related: diagnostic.related.into_iter().map(CsvLabel::from).collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsvLabel {
    pub location: CsvLocation,
    pub message: Option<String>,
}

impl From<TableLabel> for CsvLabel {
    fn from(label: TableLabel) -> Self {
        Self {
            location: CsvLocation::from(label.location),
            message: label.message,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsvLocation {
    pub file: PathBuf,
    pub sheet: Option<String>,
    pub row: Option<usize>,
    pub column: Option<usize>,
}

impl CsvLocation {
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
}

impl From<TableLocation> for CsvLocation {
    fn from(location: TableLocation) -> Self {
        Self {
            file: location.file,
            sheet: location.sheet,
            row: location.row,
            column: location.column,
        }
    }
}

/// Loads configured CSV sources into input records without building a data
/// model.
///
/// # Errors
///
/// Returns diagnostics when files, headers, or cells cannot be loaded according
/// to the schema.
pub fn collect_input_records(
    schema: &CftContainer,
    sources: &[CsvSource],
) -> Result<CsvInputRecords, CsvDiagnostics> {
    let table_sources = table_sources_from_csv(sources)?;
    collect_shared_table_input_records(schema, &table_sources)
        .map(|loaded| CsvInputRecords {
            records: loaded.records,
        })
        .map_err(CsvDiagnostics::from)
}

fn table_sources_from_csv(sources: &[CsvSource]) -> Result<Vec<TableSource>, CsvDiagnostics> {
    let mut table_sources = Vec::new();
    let mut diagnostics = Vec::new();
    for source in sources {
        match table_source_from_csv(source) {
            Ok(table_source) => table_sources.push(table_source),
            Err(err) => diagnostics.extend(err.diagnostics),
        }
    }
    if diagnostics.is_empty() {
        Ok(table_sources)
    } else {
        Err(CsvDiagnostics { diagnostics })
    }
}

fn table_source_from_csv(source: &CsvSource) -> Result<TableSource, CsvDiagnostics> {
    let text = fs::read_to_string(&source.file).map_err(|err| CsvDiagnostics {
        diagnostics: vec![CsvDiagnostic::csv(
            "CSV-READ",
            "CSV",
            format!("failed to read CSV file `{}`: {err}", source.file.display()),
            CsvLocation::new(source.file.clone()),
        )],
    })?;
    let rows = parse(&text).map_err(|err| CsvDiagnostics {
        diagnostics: vec![CsvDiagnostic::csv(
            "CSV-PARSE",
            "CSV",
            format!(
                "failed to parse CSV file `{}`: {err}",
                source.file.display()
            ),
            CsvLocation::new(source.file.clone()),
        )],
    })?;
    let configured_sheets = if source.sheets.is_empty() {
        vec![CsvSheet::new(default_sheet_name(&source.file))]
    } else {
        source.sheets.clone()
    };
    let table_sheets = configured_sheets
        .iter()
        .map(|sheet| TableSheet::new(sheet.sheet.clone(), rows.clone()))
        .collect::<Vec<_>>();
    Ok(TableSource::new(
        source.file.clone(),
        table_sheets,
        configured_sheets
            .into_iter()
            .map(TableSheetConfig::from)
            .collect(),
    ))
}

fn default_sheet_name(path: &Path) -> String {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.trim().is_empty())
        .map_or_else(|| "csv".to_string(), ToString::to_string)
}

fn table_code_to_csv(code: &str) -> String {
    code.strip_prefix("TABLE-").map_or_else(
        || code.to_string(),
        |suffix| match suffix {
            "TYPE" => "CSV-TYPE".to_string(),
            "ID" => "CSV-ID".to_string(),
            "SHEET" => "CSV-SHEET".to_string(),
            "COLUMN" => "CSV-COLUMN".to_string(),
            other => format!("CSV-{other}"),
        },
    )
}

fn table_stage_to_csv(stage: &str) -> String {
    if stage == "TABLE" {
        "CSV".to_string()
    } else {
        stage.to_string()
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct CsvLoader;

pub const CSV_LOADER_DESCRIPTOR: LoaderDescriptor = LoaderDescriptor {
    id: "csv",
    display_name: "CSV file",
    extensions: &["csv"],
    uri_schemes: &[],
    option_keys: &["sheets"],
};

impl DataLoader for CsvLoader {
    fn descriptor(&self) -> &'static LoaderDescriptor {
        &CSV_LOADER_DESCRIPTOR
    }

    fn probe(&self, source: &ProjectSourceRef<'_>) -> ProbeResult {
        if source.source_type == Some(CSV_LOADER_DESCRIPTOR.id) {
            return ProbeResult::certain();
        }
        if matches!(
            source.location,
            SourceLocationSpec::Path(path)
                if path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| CSV_LOADER_DESCRIPTOR.extensions.contains(&ext))
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
            if source.provider_id == CSV_LOADER_DESCRIPTOR.id {
                return Err(DiagnosticSet::one(Diagnostic::error(
                    "CSV-SOURCE",
                    "CSV",
                    "csv source requires `path`",
                )));
            }
            return Ok(Vec::new());
        };
        if path.is_dir() {
            return collect_csv_sources(path, source);
        }
        if is_csv_path(path) {
            let mut resolved = source.clone();
            resolved.provider_id = CSV_LOADER_DESCRIPTOR.id.to_string();
            return Ok(vec![resolved]);
        }
        Err(DiagnosticSet::one(Diagnostic::error(
            "CSV-SOURCE",
            "CSV",
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
                "CSV-SOURCE",
                "CSV",
                "csv source requires `path`",
            )));
        };
        let sheets = csv_sheets_from_options(&source.options)?;
        let csv_source = CsvSource::new(file.clone(), sheets);
        collect_input_records(ctx.schema, &[csv_source])
            .map(|loaded| LoadedRecords {
                records: loaded.records,
            })
            .map_err(csv_diagnostics_to_api)
    }
}

fn collect_csv_sources(
    dir: &Path,
    source: &ResolvedSource,
) -> Result<Vec<ResolvedSource>, DiagnosticSet> {
    let mut entries = fs::read_dir(dir)
        .map_err(|err| {
            DiagnosticSet::one(Diagnostic::error(
                "CSV-SOURCE",
                "CSV",
                format!("failed to read data source dir `{}`: {err}", dir.display()),
            ))
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| {
            DiagnosticSet::one(Diagnostic::error(
                "CSV-SOURCE",
                "CSV",
                format!("failed to read data source dir `{}`: {err}", dir.display()),
            ))
        })?;
    entries.sort_by_key(fs::DirEntry::path);

    let mut sources = Vec::new();
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            sources.extend(collect_csv_sources(&path, source)?);
        } else if is_csv_path(&path) {
            sources.push(ResolvedSource {
                provider_id: CSV_LOADER_DESCRIPTOR.id.to_string(),
                display_name: path.display().to_string(),
                location: SourceLocationSpec::Path(path),
                options: source.options.clone(),
            });
        }
    }
    Ok(sources)
}

fn is_csv_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| CSV_LOADER_DESCRIPTOR.extensions.contains(&ext))
}

fn csv_diagnostics_to_api(err: CsvDiagnostics) -> DiagnosticSet {
    DiagnosticSet {
        diagnostics: err
            .diagnostics
            .into_iter()
            .map(csv_diagnostic_to_api)
            .collect(),
    }
}

fn csv_diagnostic_to_api(diagnostic: CsvDiagnostic) -> Diagnostic {
    Diagnostic {
        code: diagnostic.code,
        stage: diagnostic.stage,
        severity: Severity::Error,
        message: diagnostic.message,
        primary: diagnostic.primary.map(csv_label_to_api),
        related: diagnostic
            .related
            .into_iter()
            .map(csv_label_to_api)
            .collect(),
    }
}

fn csv_label_to_api(label: CsvLabel) -> Label {
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
