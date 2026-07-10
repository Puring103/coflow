use crate::diagnostics::{CsvDiagnostic, CsvDiagnostics, CsvLocation};
use crate::format::parse;
use coflow_cft::CftSchemaView;
use coflow_data_model::CfdInputRecord;
use coflow_loader_table_core::{
    collect_table_input_records as collect_shared_table_input_records, TableSheet,
    TableSheetConfig, TableSource,
};
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

#[derive(Debug, Clone)]
pub struct CsvInputRecords {
    pub records: Vec<CfdInputRecord>,
}

/// Loads configured CSV sources into input records without building a data
/// model.
///
/// # Errors
///
/// Returns diagnostics when files, headers, or cells cannot be loaded according
/// to the schema.
pub fn collect_input_records(
    schema: &CftSchemaView,
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
