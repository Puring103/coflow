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
    CfdDataModel, CfdDiagnostic, CfdDiagnostics, CfdInputRecord, CfdInputValue, CfdLabel, CfdPath,
    CfdPathSegment, CfdRecordId,
};
use std::collections::BTreeMap;
use std::path::PathBuf;

const IMPORT_CONTROL_COLUMN: &str = "#";
const SKIP_IMPORT_ROW_MARKER: &str = "##";

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
    pub code: String,
    pub stage: String,
    pub message: String,
    pub source: Option<CfdDiagnostic>,
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
) -> Result<CfdDataModel, ExcelDiagnostics> {
    let loaded = collect_input_records(schema, sources)?;
    let mut builder = CfdDataModel::builder(schema);
    for record in loaded.records {
        builder.add_input_record(record);
    }
    builder
        .build()
        .map_err(|diagnostics| loaded.origins.map(diagnostics))
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
    let loaded = collect_input_records(schema, sources)?;
    let mut builder = CfdDataModel::builder(schema);
    for record in loaded.records {
        builder.add_input_record(record);
    }
    let model = builder
        .build()
        .map_err(|diagnostics| loaded.origins.clone().map(diagnostics))?;
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

#[allow(clippy::too_many_lines)]
fn collect_input_records(
    schema: &CftContainer,
    sources: &[ExcelSource],
) -> Result<LoadedInput, ExcelDiagnostics> {
    let mut records = Vec::new();
    let mut origins = ExcelOrigins::default();
    let mut diagnostics = Vec::new();
    for source in sources {
        let mut workbook = match open_workbook_auto(&source.file) {
            Ok(workbook) => workbook,
            Err(err) => {
                diagnostics.extend(excel_load_error_diagnostics(ExcelLoadError::OpenWorkbook {
                    file: source.file.clone(),
                    message: err.to_string(),
                }));
                continue;
            }
        };
        let sheet_names = workbook.sheet_names();

        for sheet in &source.sheets {
            let type_name = sheet.type_name();
            let Some(fields) = full_field_types(schema, type_name) else {
                diagnostics.extend(excel_load_error_diagnostics(ExcelLoadError::UnknownType {
                    location: Box::new(
                        ExcelLocation::new(source.file.clone()).sheet(sheet.sheet.clone()),
                    ),
                    type_name: type_name.to_string(),
                }));
                continue;
            };

            if !sheet_names.iter().any(|name| name == &sheet.sheet) {
                diagnostics.extend(excel_load_error_diagnostics(ExcelLoadError::MissingSheet {
                    file: source.file.clone(),
                    sheet: sheet.sheet.clone(),
                }));
                continue;
            }

            let range = match workbook.worksheet_range(&sheet.sheet) {
                Ok(range) => range,
                Err(err) => {
                    diagnostics.extend(excel_load_error_diagnostics(ExcelLoadError::ReadSheet {
                        location: Box::new(
                            ExcelLocation::new(source.file.clone()).sheet(sheet.sheet.clone()),
                        ),
                        message: err.to_string(),
                    }));
                    continue;
                }
            };

            if range.is_empty() {
                diagnostics.extend(excel_load_error_diagnostics(ExcelLoadError::EmptySheet {
                    location: Box::new(
                        ExcelLocation::new(source.file.clone()).sheet(sheet.sheet.clone()),
                    ),
                }));
                continue;
            }

            let (range_start_row, range_start_col) = range.start().unwrap_or((0, 0));
            let header_excel_row = range_start_row as usize + 1;
            let header_excel_col = range_start_col as usize + 1;
            let mut rows = range.rows();
            let Some(header_row) = rows.next() else {
                diagnostics.extend(excel_load_error_diagnostics(ExcelLoadError::MissingSheet {
                    file: source.file.clone(),
                    sheet: sheet.sheet.clone(),
                }));
                continue;
            };

            let resolved = match resolve_columns(
                schema,
                source,
                sheet,
                type_name,
                &fields,
                header_row,
                header_excel_row,
                header_excel_col,
            ) {
                Ok(resolved) => resolved,
                Err(sheet_diagnostics) => {
                    diagnostics.extend(sheet_diagnostics.diagnostics);
                    continue;
                }
            };
            let columns = resolved.columns;
            for (zero_based_data_row, row) in rows.enumerate() {
                if should_skip_import_row(row, resolved.control_column) {
                    continue;
                }
                if is_empty_mapped_row(row, &columns) {
                    continue;
                }
                let excel_row = range_start_row as usize + zero_based_data_row + 2;
                let mut input_fields = BTreeMap::new();
                let row_diagnostic_start = diagnostics.len();
                for column in &columns {
                    if let Some(children) = &column.expand {
                        let Some(nested) = build_expanded_object(
                            schema,
                            source,
                            sheet,
                            type_name,
                            column,
                            children,
                            row,
                            excel_row,
                            &mut diagnostics,
                        ) else {
                            continue;
                        };
                        input_fields.insert(column.field.clone(), nested);
                        continue;
                    }
                    let location = ExcelLocation::new(source.file.clone())
                        .sheet(sheet.sheet.clone())
                        .cell(excel_row, column.excel_column);
                    let Some(text) =
                        cell_text(row.get(column.index), location.clone(), &mut diagnostics)
                    else {
                        continue;
                    };
                    let parsed = match parse_cell(schema, &column.field_type, &text) {
                        Ok(parsed) => parsed,
                        Err(err) => {
                            diagnostics.extend(excel_load_error_diagnostics(
                                ExcelLoadError::CellParse {
                                    location: Box::new(location),
                                    type_name: type_name.to_string(),
                                    field: column.field.clone(),
                                    diagnostics: err,
                                },
                            ));
                            continue;
                        }
                    };
                    if let ParsedCell::Value(value) = parsed {
                        input_fields.insert(column.field.clone(), value);
                    }
                }
                if diagnostics.len() != row_diagnostic_start {
                    continue;
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
    if diagnostics.is_empty() {
        Ok(LoadedInput { records, origins })
    } else {
        Err(ExcelDiagnostics { diagnostics })
    }
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

fn excel_load_error_diagnostics(err: ExcelLoadError) -> Vec<ExcelDiagnostic> {
    match err {
        ExcelLoadError::OpenWorkbook { file, message } => vec![ExcelDiagnostic::excel(
            "EXCEL-OPEN",
            "EXCEL",
            format!("failed to open workbook `{}`: {message}", file.display()),
            ExcelLocation::new(file),
        )],
        ExcelLoadError::ReadSheet { location, message } => vec![ExcelDiagnostic::excel(
            "EXCEL-SHEET",
            "EXCEL",
            message,
            *location,
        )],
        ExcelLoadError::MissingSheet { file, sheet } => vec![ExcelDiagnostic::excel(
            "EXCEL-SHEET",
            "EXCEL",
            format!("workbook `{}` is missing sheet `{sheet}`", file.display()),
            ExcelLocation::new(file).sheet(sheet),
        )],
        ExcelLoadError::EmptySheet { location } => vec![ExcelDiagnostic::excel(
            "EXCEL-SHEET",
            "EXCEL",
            "sheet is empty",
            *location,
        )],
        ExcelLoadError::UnknownType {
            location,
            type_name,
        } => vec![ExcelDiagnostic::excel(
            "EXCEL-TYPE",
            "EXCEL",
            format!("unknown CFT type `{type_name}`"),
            *location,
        )],
        ExcelLoadError::UnknownColumn {
            location,
            type_name,
            column,
            field,
        } => vec![ExcelDiagnostic::excel(
            "EXCEL-COLUMN",
            "EXCEL",
            format!("column `{column}` maps to unknown field `{field}` on type `{type_name}`"),
            *location,
        )],
        ExcelLoadError::DuplicateFieldColumn {
            location,
            field,
            first_column,
            duplicate_column,
        } => vec![ExcelDiagnostic::excel(
            "EXCEL-COLUMN",
            "EXCEL",
            format!("field `{field}` is mapped by both `{first_column}` and `{duplicate_column}`"),
            *location,
        )],
        ExcelLoadError::CellParse {
            location,
            type_name,
            field,
            diagnostics,
        } => diagnostics
            .diagnostics
            .into_iter()
            .map(|diag| {
                ExcelDiagnostic::excel(
                    format!("CELL-{:?}", diag.code),
                    "CELL",
                    format!(
                        "failed to parse `{type_name}.{field}` cell: {}",
                        diag.message
                    ),
                    (*location).clone(),
                )
            })
            .collect(),
        ExcelLoadError::UnsupportedCellValue { location, kind } => vec![ExcelDiagnostic::excel(
            "EXCEL-CELL",
            "EXCEL",
            format!("unsupported Excel cell value `{kind}`"),
            *location,
        )],
        ExcelLoadError::DataModel(diagnostics) => diagnostics.diagnostics,
    }
}

#[derive(Debug, Clone)]
struct ResolvedColumns {
    columns: Vec<ResolvedColumn>,
    control_column: Option<usize>,
}

#[derive(Debug, Clone)]
struct ResolvedColumn {
    index: usize,
    excel_column: usize,
    field: String,
    field_type: String,
    /// When set, this column represents an `@expand` parent field that
    /// consumes additional adjacent columns. The vector lists each consumed
    /// column's source-row index, the inner field name on the expanded type,
    /// and the inner field's CFT type name.
    expand: Option<Vec<ExpandedSubColumn>>,
}

#[derive(Debug, Clone)]
struct ExpandedSubColumn {
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
            code: diagnostic.code.as_str().to_string(),
            stage: diagnostic.stage.to_string(),
            message: diagnostic.message.clone(),
            primary: diagnostic
                .primary
                .as_ref()
                .and_then(|label| self.map_label(label)),
            related: diagnostic
                .related
                .iter()
                .filter_map(|label| self.map_label(label))
                .collect(),
            source: Some(diagnostic),
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

#[allow(clippy::too_many_lines, clippy::too_many_arguments)]
fn resolve_columns(
    schema: &CftContainer,
    source: &ExcelSource,
    sheet: &ExcelSheet,
    type_name: &str,
    fields: &BTreeMap<String, String>,
    header_row: &[Data],
    header_excel_row: usize,
    header_excel_col: usize,
) -> Result<ResolvedColumns, ExcelDiagnostics> {
    let mut diagnostics = Vec::new();
    // Read the entire header row first so we can scan ahead for @expand
    // children that occupy adjacent columns.
    let mut header = Vec::with_capacity(header_row.len());
    for (index, cell) in header_row.iter().enumerate() {
        let excel_column = header_excel_col + index;
        let Some(column) = cell_text(
            Some(cell),
            ExcelLocation::new(source.file.clone())
                .sheet(sheet.sheet.clone())
                .cell(header_excel_row, excel_column),
            &mut diagnostics,
        ) else {
            continue;
        };
        header.push((index, excel_column, column.trim().to_string()));
    }

    let expand_fields = expand_field_index(schema, type_name);
    let expand_inner_order = expand_field_order_index(schema, type_name);
    let mut columns = Vec::new();
    let mut control_column = None;
    let mut seen_fields = BTreeMap::<String, String>::new();

    let mut cursor = 0;
    while cursor < header.len() {
        let (index, excel_column, column_text) = &header[cursor];
        let index = *index;
        let excel_column = *excel_column;
        let column_text = column_text.clone();
        cursor += 1;
        if column_text.is_empty() {
            continue;
        }
        if column_text == IMPORT_CONTROL_COLUMN {
            control_column = Some(index);
            continue;
        }
        let field = sheet
            .columns
            .get(&column_text)
            .map_or_else(|| column_text.clone(), Clone::clone);
        let Some(field_type) = fields.get(&field) else {
            diagnostics.extend(excel_load_error_diagnostics(
                ExcelLoadError::UnknownColumn {
                    location: Box::new(
                        ExcelLocation::new(source.file.clone())
                            .sheet(sheet.sheet.clone())
                            .cell(header_excel_row, excel_column),
                    ),
                    type_name: type_name.to_string(),
                    column: column_text,
                    field,
                },
            ));
            continue;
        };
        if let Some(first_column) = seen_fields.insert(field.clone(), column_text.clone()) {
            diagnostics.extend(excel_load_error_diagnostics(
                ExcelLoadError::DuplicateFieldColumn {
                    location: Box::new(
                        ExcelLocation::new(source.file.clone())
                            .sheet(sheet.sheet.clone())
                            .cell(header_excel_row, excel_column),
                    ),
                    field,
                    first_column,
                    duplicate_column: column_text,
                },
            ));
            continue;
        }

        let expand = if let Some(child_fields) = expand_fields.get(&field) {
            // The @expand field consumes the parent header column itself plus
            // the N-1 following data columns (where N is the inner type's
            // field count). Sub-field assignment is positional, following the
            // inner type's declared field order — adjacent header text is
            // ignored (it is typically merged-blank in source files).
            let inner_order = expand_inner_order.get(&field).cloned().unwrap_or_default();
            let mut consumed = Vec::with_capacity(inner_order.len());
            // First child uses the parent column itself.
            if let Some(first_inner) = inner_order.first() {
                let inner_ty = child_fields.get(first_inner).cloned().unwrap_or_default();
                consumed.push(ExpandedSubColumn {
                    index,
                    excel_column,
                    field: first_inner.clone(),
                    field_type: inner_ty,
                });
            }
            // Remaining children come from the columns immediately after.
            for inner_field in inner_order.iter().skip(1) {
                if cursor >= header.len() {
                    diagnostics.extend(excel_load_error_diagnostics(ExcelLoadError::UnknownColumn {
                        location: Box::new(
                            ExcelLocation::new(source.file.clone())
                                .sheet(sheet.sheet.clone())
                                .cell(header_excel_row, excel_column),
                        ),
                        type_name: type_name.to_string(),
                        column: column_text,
                        field: format!(
                            "{field} (@expand): not enough columns to cover inner field `{inner_field}`"
                        ),
                    }));
                    break;
                }
                let (next_index, next_excel_col, _next_text) = &header[cursor];
                let inner_ty = child_fields.get(inner_field).cloned().unwrap_or_default();
                consumed.push(ExpandedSubColumn {
                    index: *next_index,
                    excel_column: *next_excel_col,
                    field: inner_field.clone(),
                    field_type: inner_ty,
                });
                cursor += 1;
            }
            Some(consumed)
        } else {
            None
        };

        columns.push(ResolvedColumn {
            index,
            excel_column,
            field,
            field_type: field_type.clone(),
            expand,
        });
    }

    if diagnostics.is_empty() {
        Ok(ResolvedColumns {
            columns,
            control_column,
        })
    } else {
        Err(ExcelDiagnostics { diagnostics })
    }
}

#[allow(clippy::too_many_arguments)]
fn build_expanded_object(
    schema: &CftContainer,
    source: &ExcelSource,
    sheet: &ExcelSheet,
    parent_type: &str,
    column: &ResolvedColumn,
    children: &[ExpandedSubColumn],
    row: &[Data],
    excel_row: usize,
    diagnostics: &mut Vec<ExcelDiagnostic>,
) -> Option<CfdInputValue> {
    let mut fields = BTreeMap::new();
    let diagnostic_start = diagnostics.len();
    for child in children {
        let location = ExcelLocation::new(source.file.clone())
            .sheet(sheet.sheet.clone())
            .cell(excel_row, child.excel_column);
        let Some(text) = cell_text(row.get(child.index), location.clone(), diagnostics) else {
            continue;
        };
        let parsed = match parse_cell(schema, &child.field_type, &text) {
            Ok(parsed) => parsed,
            Err(err) => {
                diagnostics.extend(excel_load_error_diagnostics(ExcelLoadError::CellParse {
                    location: Box::new(location),
                    type_name: parent_type.to_string(),
                    field: format!("{}.{}", column.field, child.field),
                    diagnostics: err,
                }));
                continue;
            }
        };
        if let ParsedCell::Value(value) = parsed {
            fields.insert(child.field.clone(), value);
        }
    }
    if diagnostics.len() == diagnostic_start {
        Some(CfdInputValue::Object {
            actual_type: None,
            fields,
        })
    } else {
        None
    }
}

/// Returns a map from `@expand` field name -> map of inner field name to inner
/// CFT type. Inner type lookups follow the resolved field type.
fn expand_field_index(
    schema: &CftContainer,
    type_name: &str,
) -> BTreeMap<String, BTreeMap<String, String>> {
    let mut out = BTreeMap::new();
    let Some(schema_type) = schema.resolve_type(type_name) else {
        return out;
    };
    for field in &schema_type.all_fields {
        if !field
            .annotations
            .iter()
            .any(|annotation| annotation.name == "expand")
        {
            continue;
        }
        let Some(inner_type) = schema.resolve_type(&field.ty) else {
            continue;
        };
        let inner_fields = inner_type
            .all_fields
            .iter()
            .map(|inner| (inner.name.clone(), inner.ty.clone()))
            .collect();
        out.insert(field.name.clone(), inner_fields);
    }
    out
}

/// Returns a map from `@expand` field name -> ordered list of inner field
/// names (declaration order on the expanded type). Excel data is read
/// positionally in this order.
fn expand_field_order_index(
    schema: &CftContainer,
    type_name: &str,
) -> BTreeMap<String, Vec<String>> {
    let mut out = BTreeMap::new();
    let Some(schema_type) = schema.resolve_type(type_name) else {
        return out;
    };
    for field in &schema_type.all_fields {
        if !field
            .annotations
            .iter()
            .any(|annotation| annotation.name == "expand")
        {
            continue;
        }
        let Some(inner_type) = schema.resolve_type(&field.ty) else {
            continue;
        };
        let order = inner_type
            .all_fields
            .iter()
            .map(|inner| inner.name.clone())
            .collect();
        out.insert(field.name.clone(), order);
    }
    out
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
    columns.iter().all(|column| {
        column.expand.as_ref().map_or_else(
            || row.get(column.index).is_none_or(is_empty_cell),
            |children| {
                children
                    .iter()
                    .all(|child| row.get(child.index).is_none_or(is_empty_cell))
            },
        )
    })
}

fn should_skip_import_row(row: &[Data], control_column: Option<usize>) -> bool {
    let Some(index) = control_column else {
        return false;
    };
    row.get(index).is_some_and(|cell| match cell {
        Data::String(value) => value.trim() == SKIP_IMPORT_ROW_MARKER,
        _ => false,
    })
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
            diagnostics.extend(excel_load_error_diagnostics(
                ExcelLoadError::UnsupportedCellValue {
                    location: Box::new(location),
                    kind: format!("DateTime({value})"),
                },
            ));
            None
        }
        Some(Data::DateTimeIso(value)) => {
            diagnostics.extend(excel_load_error_diagnostics(
                ExcelLoadError::UnsupportedCellValue {
                    location: Box::new(location),
                    kind: format!("DateTimeIso({value})"),
                },
            ));
            None
        }
        Some(Data::DurationIso(value)) => {
            diagnostics.extend(excel_load_error_diagnostics(
                ExcelLoadError::UnsupportedCellValue {
                    location: Box::new(location),
                    kind: format!("DurationIso({value})"),
                },
            ));
            None
        }
        Some(Data::Error(value)) => {
            diagnostics.extend(excel_load_error_diagnostics(
                ExcelLoadError::UnsupportedCellValue {
                    location: Box::new(location),
                    kind: format!("Error({value})"),
                },
            ));
            None
        }
    }
}

fn is_whole_float(value: f64) -> bool {
    value.is_finite() && value.fract().abs() < f64::EPSILON
}
