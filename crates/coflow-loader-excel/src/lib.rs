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
#![allow(clippy::missing_const_for_fn)]

use calamine::{open_workbook_auto, Data, Reader};
use coflow_cell_value::{parse_cell, CellValueDiagnostics, ParsedCell};
use coflow_cft::CftContainer;
use coflow_data_model::{
    CfdDataModel, CfdDiagnostic, CfdDiagnostics, CfdInputRecord, CfdLabel, CfdPath, CfdPathSegment,
    CfdRecordId,
};
use std::collections::BTreeMap;
use std::path::PathBuf;

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
    pub columns: BTreeMap<String, String>,
}

impl ExcelSheet {
    #[must_use]
    pub fn new(sheet: impl Into<String>) -> Self {
        Self {
            sheet: sheet.into(),
            type_name: None,
            columns: BTreeMap::new(),
        }
    }

    #[must_use]
    pub fn with_type(mut self, type_name: impl Into<String>) -> Self {
        self.type_name = Some(type_name.into());
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExcelDiagnostic {
    pub source: CfdDiagnostic,
    pub primary: Option<ExcelLabel>,
    pub related: Vec<ExcelLabel>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExcelLabel {
    pub location: ExcelLocation,
    pub message: Option<String>,
}

#[derive(Debug)]
pub enum ExcelLoadError {
    OpenWorkbook {
        file: PathBuf,
        message: String,
    },
    ReadSheet {
        location: Box<ExcelLocation>,
        message: String,
    },
    MissingSheet {
        file: PathBuf,
        sheet: String,
    },
    EmptySheet {
        location: Box<ExcelLocation>,
    },
    UnknownType {
        location: Box<ExcelLocation>,
        type_name: String,
    },
    UnknownColumn {
        location: Box<ExcelLocation>,
        type_name: String,
        column: String,
        field: String,
    },
    DuplicateFieldColumn {
        location: Box<ExcelLocation>,
        field: String,
        first_column: String,
        duplicate_column: String,
    },
    CellParse {
        location: Box<ExcelLocation>,
        type_name: String,
        field: String,
        diagnostics: CellValueDiagnostics,
    },
    UnsupportedCellValue {
        location: Box<ExcelLocation>,
        kind: String,
    },
    DataModel(ExcelDiagnostics),
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
) -> Result<CfdDataModel, ExcelLoadError> {
    let loaded = collect_input_records(schema, sources)?;
    let mut builder = CfdDataModel::builder(schema);
    for record in loaded.records {
        builder.add_input_record(record);
    }
    builder
        .build()
        .map_err(|diagnostics| ExcelLoadError::DataModel(loaded.origins.map(diagnostics)))
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
) -> Result<ExcelLoadOutput, ExcelLoadError> {
    let loaded = collect_input_records(schema, sources)?;
    let mut builder = CfdDataModel::builder(schema);
    for record in loaded.records {
        builder.add_input_record(record);
    }
    let model = builder.build().map_err(|diagnostics| {
        ExcelLoadError::DataModel(loaded.origins.clone().map(diagnostics))
    })?;
    let check_diagnostics = coflow_checker::run_checks(schema, &model)
        .err()
        .map(|diagnostics| loaded.origins.map(diagnostics));
    Ok(ExcelLoadOutput {
        model,
        check_diagnostics,
    })
}

#[derive(Debug, Clone)]
struct LoadedInput {
    records: Vec<CfdInputRecord>,
    origins: ExcelOrigins,
}

fn collect_input_records(
    schema: &CftContainer,
    sources: &[ExcelSource],
) -> Result<LoadedInput, ExcelLoadError> {
    let mut records = Vec::new();
    let mut origins = ExcelOrigins::default();
    for source in sources {
        let mut workbook =
            open_workbook_auto(&source.file).map_err(|err| ExcelLoadError::OpenWorkbook {
                file: source.file.clone(),
                message: err.to_string(),
            })?;
        let sheet_names = workbook.sheet_names();

        for sheet in &source.sheets {
            let type_name = sheet.type_name();
            let fields =
                full_field_types(schema, type_name).ok_or_else(|| ExcelLoadError::UnknownType {
                    location: Box::new(
                        ExcelLocation::new(source.file.clone()).sheet(sheet.sheet.clone()),
                    ),
                    type_name: type_name.to_string(),
                })?;

            if !sheet_names.iter().any(|name| name == &sheet.sheet) {
                return Err(ExcelLoadError::MissingSheet {
                    file: source.file.clone(),
                    sheet: sheet.sheet.clone(),
                });
            }

            let range = workbook.worksheet_range(&sheet.sheet).map_err(|err| {
                ExcelLoadError::ReadSheet {
                    location: Box::new(
                        ExcelLocation::new(source.file.clone()).sheet(sheet.sheet.clone()),
                    ),
                    message: err.to_string(),
                }
            })?;

            if range.is_empty() {
                return Err(ExcelLoadError::EmptySheet {
                    location: Box::new(
                        ExcelLocation::new(source.file.clone()).sheet(sheet.sheet.clone()),
                    ),
                });
            }

            let (range_start_row, range_start_col) = range.start().unwrap_or((0, 0));
            let header_excel_row = range_start_row as usize + 1;
            let header_excel_col = range_start_col as usize + 1;
            let mut rows = range.rows();
            let Some(header_row) = rows.next() else {
                return Err(ExcelLoadError::MissingSheet {
                    file: source.file.clone(),
                    sheet: sheet.sheet.clone(),
                });
            };

            let columns = resolve_columns(
                source,
                sheet,
                type_name,
                &fields,
                header_row,
                header_excel_row,
                header_excel_col,
            )?;
            for (zero_based_data_row, row) in rows.enumerate() {
                if is_empty_mapped_row(row, &columns) {
                    continue;
                }
                let excel_row = range_start_row as usize + zero_based_data_row + 2;
                let mut input_fields = BTreeMap::new();
                for column in &columns {
                    let location = ExcelLocation::new(source.file.clone())
                        .sheet(sheet.sheet.clone())
                        .cell(excel_row, column.excel_column);
                    let text = cell_text(row.get(column.index), location.clone())?;
                    let parsed = parse_cell(schema, &column.field_type, &text).map_err(|err| {
                        ExcelLoadError::CellParse {
                            location: Box::new(location),
                            type_name: type_name.to_string(),
                            field: column.field.clone(),
                            diagnostics: err,
                        }
                    })?;
                    if let ParsedCell::Value(value) = parsed {
                        input_fields.insert(column.field.clone(), value);
                    }
                }
                origins.push(ExcelRecordOrigin::new(
                    source.file.clone(),
                    sheet.sheet.clone(),
                    excel_row,
                    &columns,
                ));
                records.push(CfdInputRecord::new(type_name, input_fields));
            }
        }
    }
    Ok(LoadedInput { records, origins })
}

#[derive(Debug, Clone)]
struct ResolvedColumn {
    index: usize,
    excel_column: usize,
    field: String,
    field_type: String,
}

#[derive(Debug, Clone, Default)]
struct ExcelOrigins {
    records: Vec<ExcelRecordOrigin>,
}

impl ExcelOrigins {
    fn push(&mut self, origin: ExcelRecordOrigin) {
        self.records.push(origin);
    }

    fn map(&self, diagnostics: CfdDiagnostics) -> ExcelDiagnostics {
        ExcelDiagnostics {
            diagnostics: diagnostics
                .diagnostics
                .into_iter()
                .map(|diagnostic| self.map_diagnostic(diagnostic))
                .collect(),
        }
    }

    fn map_diagnostic(&self, diagnostic: CfdDiagnostic) -> ExcelDiagnostic {
        ExcelDiagnostic {
            primary: diagnostic
                .primary
                .as_ref()
                .and_then(|label| self.map_label(label)),
            related: diagnostic
                .related
                .iter()
                .filter_map(|label| self.map_label(label))
                .collect(),
            source: diagnostic,
        }
    }

    fn map_label(&self, label: &CfdLabel) -> Option<ExcelLabel> {
        let record = label.record?;
        let origin = self.record(record)?;
        Some(ExcelLabel {
            location: origin.location_for_path(&label.path),
            message: label.message.clone(),
        })
    }

    fn record(&self, record: CfdRecordId) -> Option<&ExcelRecordOrigin> {
        self.records.get(record.index())
    }
}

#[derive(Debug, Clone)]
struct ExcelRecordOrigin {
    file: PathBuf,
    sheet: String,
    row: usize,
    fields: BTreeMap<String, usize>,
}

impl ExcelRecordOrigin {
    fn new(file: PathBuf, sheet: String, row: usize, columns: &[ResolvedColumn]) -> Self {
        Self {
            file,
            sheet,
            row,
            fields: columns
                .iter()
                .map(|column| (column.field.clone(), column.excel_column))
                .collect(),
        }
    }

    fn location_for_path(&self, path: &CfdPath) -> ExcelLocation {
        let column = root_field(path).and_then(|field| self.fields.get(field).copied());
        ExcelLocation::new(self.file.clone())
            .sheet(self.sheet.clone())
            .with_row(self.row)
            .with_column(column)
    }
}

fn resolve_columns(
    source: &ExcelSource,
    sheet: &ExcelSheet,
    type_name: &str,
    fields: &BTreeMap<String, String>,
    header_row: &[Data],
    header_excel_row: usize,
    header_excel_col: usize,
) -> Result<Vec<ResolvedColumn>, ExcelLoadError> {
    let mut columns = Vec::new();
    let mut seen_fields = BTreeMap::<String, String>::new();

    for (index, cell) in header_row.iter().enumerate() {
        let excel_column = header_excel_col + index;
        let column = cell_text(
            Some(cell),
            ExcelLocation::new(source.file.clone())
                .sheet(sheet.sheet.clone())
                .cell(header_excel_row, excel_column),
        )?;
        let column = column.trim();
        if column.is_empty() {
            continue;
        }
        let field = sheet
            .columns
            .get(column)
            .map_or_else(|| column.to_string(), Clone::clone);
        let Some(field_type) = fields.get(&field) else {
            return Err(ExcelLoadError::UnknownColumn {
                location: Box::new(
                    ExcelLocation::new(source.file.clone())
                        .sheet(sheet.sheet.clone())
                        .cell(header_excel_row, excel_column),
                ),
                type_name: type_name.to_string(),
                column: column.to_string(),
                field,
            });
        };
        if let Some(first_column) = seen_fields.insert(field.clone(), column.to_string()) {
            return Err(ExcelLoadError::DuplicateFieldColumn {
                location: Box::new(
                    ExcelLocation::new(source.file.clone())
                        .sheet(sheet.sheet.clone())
                        .cell(header_excel_row, excel_column),
                ),
                field,
                first_column,
                duplicate_column: column.to_string(),
            });
        }
        columns.push(ResolvedColumn {
            index,
            excel_column,
            field,
            field_type: field_type.clone(),
        });
    }

    Ok(columns)
}

fn full_field_types(schema: &CftContainer, type_name: &str) -> Option<BTreeMap<String, String>> {
    let schema_type = schema.resolve_type(type_name)?;
    Some(
        schema_type
            .all_fields
            .iter()
            .map(|field| (field.name.clone(), field.ty.clone()))
            .collect(),
    )
}

fn root_field(path: &CfdPath) -> Option<&str> {
    path.segments.iter().find_map(|segment| match segment {
        CfdPathSegment::Field(name) => Some(name.as_str()),
        CfdPathSegment::Index(_) | CfdPathSegment::DictKey(_) => None,
    })
}

fn is_empty_mapped_row(row: &[Data], columns: &[ResolvedColumn]) -> bool {
    columns
        .iter()
        .all(|column| row.get(column.index).is_none_or(is_empty_cell))
}

fn is_empty_cell(cell: &Data) -> bool {
    match cell {
        Data::Empty => true,
        Data::String(value) => value.trim().is_empty(),
        Data::Float(_)
        | Data::Int(_)
        | Data::Bool(_)
        | Data::DateTime(_)
        | Data::DateTimeIso(_)
        | Data::DurationIso(_)
        | Data::Error(_) => false,
    }
}

fn cell_text(cell: Option<&Data>, location: ExcelLocation) -> Result<String, ExcelLoadError> {
    match cell {
        None | Some(Data::Empty) => Ok(String::new()),
        Some(Data::String(value)) => Ok(value.clone()),
        Some(Data::Float(value)) if is_whole_float(*value) => Ok(format!("{value:.0}")),
        Some(Data::Float(value)) => Ok(value.to_string()),
        Some(Data::Int(value)) => Ok(value.to_string()),
        Some(Data::Bool(value)) => Ok(value.to_string()),
        Some(Data::DateTime(value)) => Err(ExcelLoadError::UnsupportedCellValue {
            location: Box::new(location),
            kind: format!("DateTime({value})"),
        }),
        Some(Data::DateTimeIso(value)) => Err(ExcelLoadError::UnsupportedCellValue {
            location: Box::new(location),
            kind: format!("DateTimeIso({value})"),
        }),
        Some(Data::DurationIso(value)) => Err(ExcelLoadError::UnsupportedCellValue {
            location: Box::new(location),
            kind: format!("DurationIso({value})"),
        }),
        Some(Data::Error(value)) => Err(ExcelLoadError::UnsupportedCellValue {
            location: Box::new(location),
            kind: format!("Error({value})"),
        }),
    }
}

fn is_whole_float(value: f64) -> bool {
    value.is_finite() && value.fract().abs() < f64::EPSILON
}
