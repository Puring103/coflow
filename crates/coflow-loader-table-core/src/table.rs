//! Excel-like table loader for Coflow data models.
//!
//! Source-specific loaders should convert their input into [`TableSource`]
//! values, then use this crate for schema-guided row, key, column, and cell
//! parsing.

use coflow_cft::{record_key_ident_error, CftContainer};
use coflow_data_model::{
    CfdDiagnostic, CfdDiagnostics, CfdInputRecord, CfdInputValue, CfdLabel, CfdPath,
    CfdPathSegment, RecordOrigin, SourceDocument, SourceLocation,
};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::cell_value::{parse_cell, CellValueDiagnostics, ParsedCell};

const IMPORT_CONTROL_COLUMN: &str = "#";
const SKIP_IMPORT_ROW_MARKER: &str = "##";
const DEFAULT_KEY_COLUMN: &str = "id";
const DEFAULT_KEY_COLUMN_ALIASES: &[&str] = &["id", "Id", "ID"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableSheetConfig {
    pub sheet: String,
    pub type_name: Option<String>,
    pub key: Option<String>,
    pub columns: BTreeMap<String, String>,
}

impl TableSheetConfig {
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableSource {
    pub name: PathBuf,
    pub document: SourceDocument,
    pub sheets: Vec<TableSheet>,
    pub configs: Vec<TableSheetConfig>,
}

impl TableSource {
    #[must_use]
    pub fn new(
        name: impl Into<PathBuf>,
        sheets: Vec<TableSheet>,
        configs: Vec<TableSheetConfig>,
    ) -> Self {
        let name = name.into();
        Self {
            document: SourceDocument::Local(name.clone()),
            name,
            sheets,
            configs,
        }
    }

    #[must_use]
    pub fn remote(
        name: impl Into<PathBuf>,
        document: impl Into<String>,
        sheets: Vec<TableSheet>,
        configs: Vec<TableSheetConfig>,
    ) -> Self {
        Self {
            name: name.into(),
            document: SourceDocument::Remote(document.into()),
            sheets,
            configs,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableSheet {
    pub name: String,
    pub rows: Vec<Vec<String>>,
    pub start_row: usize,
    pub start_column: usize,
}

impl TableSheet {
    #[must_use]
    pub fn new(name: impl Into<String>, rows: Vec<Vec<String>>) -> Self {
        Self {
            name: name.into(),
            rows,
            start_row: 1,
            start_column: 1,
        }
    }

    #[must_use]
    pub fn with_start(mut self, row: usize, column: usize) -> Self {
        self.start_row = row;
        self.start_column = column;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableDiagnostics {
    pub diagnostics: Vec<TableDiagnostic>,
}

#[derive(Debug, Clone)]
pub struct TableInputRecords {
    /// Each record carries its own [`RecordOrigin`] (a [`RecordOrigin::Table`]
    /// variant). Diagnostics produced before data-model diagnostics are mapped
    /// can use the records' origins to resolve labels back to source cells.
    pub records: Vec<CfdInputRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableDiagnostic {
    pub code: String,
    pub stage: String,
    pub message: String,
    pub source: Option<CfdDiagnostic>,
    pub primary: Option<TableLabel>,
    pub related: Vec<TableLabel>,
}

impl TableDiagnostic {
    #[must_use]
    pub fn table(
        code: impl Into<String>,
        stage: impl Into<String>,
        message: impl Into<String>,
        location: TableLocation,
    ) -> Self {
        Self {
            code: code.into(),
            stage: stage.into(),
            message: message.into(),
            source: None,
            primary: Some(TableLabel {
                location,
                message: None,
            }),
            related: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableLabel {
    pub location: TableLocation,
    pub message: Option<String>,
}

#[derive(Debug)]
enum TableLoadError {
    MissingSheet {
        file: PathBuf,
        sheet: String,
    },
    EmptySheet {
        location: Box<TableLocation>,
    },
    UnknownType {
        location: Box<TableLocation>,
        type_name: String,
    },
    UnknownColumn {
        location: Box<TableLocation>,
        type_name: String,
        column: String,
        field: String,
    },
    MissingColumn {
        location: Box<TableLocation>,
        type_name: String,
        field: String,
    },
    DuplicateFieldColumn {
        location: Box<TableLocation>,
        field: String,
        first_column: String,
        duplicate_column: String,
    },
    MissingKeyColumn {
        location: Box<TableLocation>,
        type_name: String,
        key: String,
    },
    DuplicateKeyColumn {
        location: Box<TableLocation>,
        key: String,
    },
    UnexpectedExpandHeader {
        location: Box<TableLocation>,
        parent_field: String,
        expected_field: String,
        header: String,
    },
    EmptyIdCell {
        location: Box<TableLocation>,
    },
    InvalidIdCell {
        location: Box<TableLocation>,
        key: String,
        reason: String,
    },
    CellParse {
        location: Box<TableLocation>,
        type_name: String,
        field: String,
        diagnostics: CellValueDiagnostics,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableLocation {
    pub file: PathBuf,
    pub sheet: Option<String>,
    pub row: Option<usize>,
    pub column: Option<usize>,
}

impl From<TableLocation> for SourceLocation {
    fn from(location: TableLocation) -> Self {
        Self::TableCell {
            path: location.file,
            sheet: location.sheet,
            row: location.row.unwrap_or(1),
            column: location.column.unwrap_or(1),
        }
    }
}

impl TableLocation {
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

/// Loads table sources into input records without building a data model.
///
/// # Errors
///
/// Returns diagnostics when sheets, headers, keys, or cells cannot be loaded
/// according to the schema.
#[allow(clippy::too_many_lines)]
pub fn collect_table_input_records(
    schema: &CftContainer,
    sources: &[TableSource],
) -> Result<TableInputRecords, TableDiagnostics> {
    let mut records: Vec<CfdInputRecord> = Vec::new();
    let mut diagnostics = Vec::new();
    for source in sources {
        let sheet_names = source
            .sheets
            .iter()
            .map(|sheet| sheet.name.clone())
            .collect::<Vec<_>>();

        let configured_sheets = if source.configs.is_empty() {
            sheet_names
                .iter()
                .map(|sheet| TableSheetConfig::new(sheet.clone()))
                .collect::<Vec<_>>()
        } else {
            source.configs.clone()
        };

        for sheet in &configured_sheets {
            let type_name = sheet.type_name();
            let Some(fields) = full_field_types(schema, type_name) else {
                diagnostics.extend(table_load_error_diagnostics(TableLoadError::UnknownType {
                    location: Box::new(
                        TableLocation::new(source.name.clone()).sheet(sheet.sheet.clone()),
                    ),
                    type_name: type_name.to_string(),
                }));
                continue;
            };

            let Some(table_sheet) = source
                .sheets
                .iter()
                .find(|candidate| candidate.name == sheet.sheet)
            else {
                diagnostics.extend(table_load_error_diagnostics(TableLoadError::MissingSheet {
                    file: source.name.clone(),
                    sheet: sheet.sheet.clone(),
                }));
                continue;
            };

            if table_sheet.rows.is_empty() {
                diagnostics.extend(table_load_error_diagnostics(TableLoadError::EmptySheet {
                    location: Box::new(
                        TableLocation::new(source.name.clone()).sheet(sheet.sheet.clone()),
                    ),
                }));
                continue;
            }

            let header_excel_row = table_sheet.start_row;
            let header_excel_col = table_sheet.start_column;
            let mut rows = table_sheet.rows.iter();
            let Some(header_row) = rows.next() else {
                diagnostics.extend(table_load_error_diagnostics(TableLoadError::MissingSheet {
                    file: source.name.clone(),
                    sheet: sheet.sheet.clone(),
                }));
                continue;
            };

            let resolved = match resolve_columns(
                schema,
                &source.name,
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
            let id_column = resolved.id_column;
            for (zero_based_data_row, row) in rows.enumerate() {
                if should_skip_import_row(row, resolved.control_column) {
                    continue;
                }
                if is_empty_mapped_row(row, &columns, &id_column) {
                    continue;
                }
                let excel_row = table_sheet.start_row + zero_based_data_row + 1;
                let mut input_fields = BTreeMap::new();
                let row_diagnostic_start = diagnostics.len();
                let id_location = TableLocation::new(source.name.clone())
                    .sheet(sheet.sheet.clone())
                    .cell(excel_row, id_column.excel_column);
                let record_key = table_cell_text(row.get(id_column.index)).trim().to_string();
                if record_key.is_empty() {
                    diagnostics.extend(table_load_error_diagnostics(TableLoadError::EmptyIdCell {
                        location: Box::new(id_location),
                    }));
                } else if let Some(reason) = record_key_ident_error(&record_key) {
                    diagnostics.extend(table_load_error_diagnostics(
                        TableLoadError::InvalidIdCell {
                            location: Box::new(id_location),
                            key: record_key.clone(),
                            reason,
                        },
                    ));
                }
                for column in &columns {
                    if let Some(children) = &column.expand {
                        let Some(nested) = build_expanded_object(
                            schema,
                            &source.name,
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
                    let location = TableLocation::new(source.name.clone())
                        .sheet(sheet.sheet.clone())
                        .cell(excel_row, column.excel_column);
                    let text = table_cell_text(row.get(column.index));
                    let parsed = match parse_cell(schema, &column.field_type, &text) {
                        Ok(parsed) => parsed,
                        Err(err) => {
                            diagnostics.extend(table_load_error_diagnostics(
                                TableLoadError::CellParse {
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
                let record_origin = build_record_origin(
                    source.document.clone(),
                    sheet.sheet.clone(),
                    excel_row,
                    &columns,
                    id_column.excel_column,
                );
                records.push(
                    CfdInputRecord::new(record_key, type_name, input_fields)
                        .with_origin(record_origin),
                );
            }
        }
    }
    if diagnostics.is_empty() {
        Ok(TableInputRecords { records })
    } else {
        Err(TableDiagnostics { diagnostics })
    }
}

#[allow(clippy::too_many_lines)]
fn table_load_error_diagnostics(err: TableLoadError) -> Vec<TableDiagnostic> {
    match err {
        TableLoadError::MissingSheet { file, sheet } => vec![TableDiagnostic::table(
            "TABLE-SHEET",
            "TABLE",
            format!(
                "table source `{}` is missing sheet `{sheet}`",
                file.display()
            ),
            TableLocation::new(file).sheet(sheet),
        )],
        TableLoadError::EmptySheet { location } => vec![TableDiagnostic::table(
            "TABLE-SHEET",
            "TABLE",
            "sheet is empty",
            *location,
        )],
        TableLoadError::UnknownType {
            location,
            type_name,
        } => vec![TableDiagnostic::table(
            "TABLE-TYPE",
            "TABLE",
            format!("unknown CFT type `{type_name}`"),
            *location,
        )],
        TableLoadError::UnknownColumn {
            location,
            type_name,
            column,
            field,
        } => vec![TableDiagnostic::table(
            "TABLE-COLUMN",
            "TABLE",
            format!("column `{column}` maps to unknown field `{field}` on type `{type_name}`"),
            *location,
        )],
        TableLoadError::MissingColumn {
            location,
            type_name,
            field,
        } => vec![TableDiagnostic::table(
            "TABLE-COLUMN",
            "TABLE",
            format!("sheet for type `{type_name}` is missing column for field `{field}`"),
            *location,
        )],
        TableLoadError::DuplicateFieldColumn {
            location,
            field,
            first_column,
            duplicate_column,
        } => vec![TableDiagnostic::table(
            "TABLE-COLUMN",
            "TABLE",
            format!("field `{field}` is mapped by both `{first_column}` and `{duplicate_column}`"),
            *location,
        )],
        TableLoadError::MissingKeyColumn {
            location,
            type_name,
            key,
        } => vec![TableDiagnostic::table(
            "TABLE-ID",
            "TABLE",
            format!("sheet for type `{type_name}` must contain key column `{key}`"),
            *location,
        )],
        TableLoadError::DuplicateKeyColumn { location, key } => vec![TableDiagnostic::table(
            "TABLE-COLUMN",
            "TABLE",
            format!("key column `{key}` is mapped more than once"),
            *location,
        )],
        TableLoadError::UnexpectedExpandHeader {
            location,
            parent_field,
            expected_field,
            header,
        } => vec![TableDiagnostic::table(
            "TABLE-COLUMN",
            "TABLE",
            format!(
                "@expand field `{parent_field}` expected adjacent column for `{expected_field}` \
                 to have an empty header, found `{header}`"
            ),
            *location,
        )],
        TableLoadError::EmptyIdCell { location } => vec![TableDiagnostic::table(
            "TABLE-ID",
            "TABLE",
            "record key cell is empty",
            *location,
        )],
        TableLoadError::InvalidIdCell {
            location,
            key,
            reason,
        } => vec![TableDiagnostic::table(
            "TABLE-ID",
            "TABLE",
            format!("invalid record key `{key}`: {reason}"),
            *location,
        )],
        TableLoadError::CellParse {
            location,
            type_name,
            field,
            diagnostics,
        } => diagnostics
            .diagnostics
            .iter()
            .map(|diagnostic| {
                TableDiagnostic::table(
                    format!("CELL-{:?}", diagnostic.code),
                    "CELL",
                    format!("{} while parsing `{type_name}.{field}`", diagnostic.message),
                    (*location).clone(),
                )
            })
            .collect(),
    }
}

#[derive(Debug, Clone)]
struct ResolvedColumns {
    columns: Vec<ResolvedColumn>,
    id_column: IdColumn,
    control_column: Option<usize>,
}

#[derive(Debug, Clone)]
struct IdColumn {
    index: usize,
    excel_column: usize,
}

#[derive(Debug, Clone)]
struct ResolvedColumn {
    index: usize,
    excel_column: usize,
    field: String,
    field_type: String,
    expand: Option<Vec<ExpandedSubColumn>>,
}

#[derive(Debug, Clone)]
struct ExpandedSubColumn {
    index: usize,
    excel_column: usize,
    field: String,
    field_type: String,
}

/// Build a [`RecordOrigin::Table`] for a row, using the resolved columns to
/// produce a `field_path → column` map.
fn build_record_origin(
    document: SourceDocument,
    sheet: String,
    row: usize,
    columns: &[ResolvedColumn],
    id_column: usize,
) -> RecordOrigin {
    let mut field_columns = BTreeMap::new();
    for column in columns {
        field_columns.insert(vec![column.field.clone()], column.excel_column);
        if let Some(children) = &column.expand {
            for child in children {
                field_columns.insert(
                    vec![column.field.clone(), child.field.clone()],
                    child.excel_column,
                );
            }
        }
    }
    RecordOrigin::Table {
        document,
        sheet,
        row,
        id_column,
        field_columns,
    }
}

/// Map [`CfdDiagnostics`] to per-row [`TableDiagnostic`] using a slice of
/// record origins (typically extracted from the input records via
/// [`crate::origins_of`]).
#[must_use]
pub fn map_table_diagnostics(
    diagnostics: CfdDiagnostics,
    origins: &[RecordOrigin],
) -> TableDiagnostics {
    TableDiagnostics {
        diagnostics: diagnostics
            .diagnostics
            .into_iter()
            .map(|diagnostic| {
                let primary = diagnostic
                    .primary
                    .as_ref()
                    .and_then(|label| map_label_to_table(label, origins));
                let related = diagnostic
                    .related
                    .iter()
                    .filter_map(|label| map_label_to_table(label, origins))
                    .collect();
                TableDiagnostic {
                    code: diagnostic.code.as_str().to_string(),
                    stage: diagnostic.stage.to_string(),
                    message: diagnostic.message.clone(),
                    primary,
                    related,
                    source: Some(diagnostic),
                }
            })
            .collect(),
    }
}

#[must_use]
pub fn map_label_to_table(label: &CfdLabel, origins: &[RecordOrigin]) -> Option<TableLabel> {
    let record = label.record?;
    let origin = origins.get(record.index())?;
    let RecordOrigin::Table {
        document,
        sheet,
        row,
        id_column,
        field_columns,
    } = origin
    else {
        return None;
    };
    let column = path_column(&label.path, field_columns).or_else(|| {
        root_field(&label.path).and_then(|field| (field == "id").then_some(*id_column))
    });
    let name = match document {
        SourceDocument::Local(p) => p.clone(),
        SourceDocument::Remote(doc) => PathBuf::from(doc),
    };
    Some(TableLabel {
        location: TableLocation::new(name)
            .sheet(sheet.clone())
            .with_row(*row)
            .with_column(column),
        message: label.message.clone(),
    })
}

#[allow(clippy::too_many_lines, clippy::too_many_arguments)]
fn resolve_columns(
    schema: &CftContainer,
    source_name: &Path,
    sheet: &TableSheetConfig,
    type_name: &str,
    fields: &BTreeMap<String, String>,
    header_row: &[String],
    header_excel_row: usize,
    header_excel_col: usize,
) -> Result<ResolvedColumns, TableDiagnostics> {
    let mut diagnostics = Vec::new();
    let mut header = Vec::with_capacity(header_row.len());
    for (index, cell) in header_row.iter().enumerate() {
        let excel_column = header_excel_col + index;
        let column = table_cell_text(Some(cell));
        header.push((index, excel_column, column.trim().to_string()));
    }

    let expand_fields = expand_field_index(schema, type_name);
    let expand_inner_order = expand_field_order_index(schema, type_name);
    let mut columns = Vec::new();
    let mut id_column = None::<(usize, usize, String)>;
    let mut control_column = None;
    let mut seen_fields = BTreeMap::<String, String>::new();
    let key_column = sheet.key_column().to_string();
    let has_explicit_key = sheet.key.is_some();

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
        if is_key_column(&column_text, &field, &key_column, has_explicit_key) {
            if fields.contains_key(&field) {
                seen_fields.insert(field.clone(), column_text.clone());
            }
            if id_column
                .replace((index, excel_column, column_text.clone()))
                .is_some()
            {
                diagnostics.extend(table_load_error_diagnostics(
                    TableLoadError::DuplicateKeyColumn {
                        location: Box::new(
                            TableLocation::new(source_name.to_path_buf())
                                .sheet(sheet.sheet.clone())
                                .cell(header_excel_row, excel_column),
                        ),
                        key: key_column.clone(),
                    },
                ));
            }
            continue;
        }
        let Some(field_type) = fields.get(&field) else {
            diagnostics.extend(table_load_error_diagnostics(
                TableLoadError::UnknownColumn {
                    location: Box::new(
                        TableLocation::new(source_name.to_path_buf())
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
            diagnostics.extend(table_load_error_diagnostics(
                TableLoadError::DuplicateFieldColumn {
                    location: Box::new(
                        TableLocation::new(source_name.to_path_buf())
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

        let expand = expand_fields.get(&field).map(|child_fields| {
            let inner_order = expand_inner_order.get(&field).cloned().unwrap_or_default();
            let mut consumed = Vec::with_capacity(inner_order.len());
            if let Some(first_inner) = inner_order.first() {
                let inner_ty = child_fields.get(first_inner).cloned().unwrap_or_default();
                consumed.push(ExpandedSubColumn {
                    index,
                    excel_column,
                    field: first_inner.clone(),
                    field_type: inner_ty,
                });
            }
            for inner_field in inner_order.iter().skip(1) {
                if cursor >= header.len() {
                    diagnostics.extend(table_load_error_diagnostics(
                        TableLoadError::UnknownColumn {
                            location: Box::new(
                                TableLocation::new(source_name.to_path_buf())
                                    .sheet(sheet.sheet.clone())
                                    .cell(header_excel_row, excel_column),
                            ),
                            type_name: type_name.to_string(),
                            column: column_text.clone(),
                            field: format!(
                                "{field} (@expand): not enough columns to cover inner field `{inner_field}`"
                            ),
                        },
                    ));
                    break;
                }
                let (next_index, next_excel_col, next_text) = &header[cursor];
                if !next_text.is_empty() {
                    diagnostics.extend(table_load_error_diagnostics(
                        TableLoadError::UnexpectedExpandHeader {
                            location: Box::new(
                                TableLocation::new(source_name.to_path_buf())
                                    .sheet(sheet.sheet.clone())
                                    .cell(header_excel_row, *next_excel_col),
                            ),
                            parent_field: field.clone(),
                            expected_field: inner_field.clone(),
                            header: next_text.clone(),
                        },
                    ));
                }
                let inner_ty = child_fields.get(inner_field).cloned().unwrap_or_default();
                consumed.push(ExpandedSubColumn {
                    index: *next_index,
                    excel_column: *next_excel_col,
                    field: inner_field.clone(),
                    field_type: inner_ty,
                });
                cursor += 1;
            }
            consumed
        });

        columns.push(ResolvedColumn {
            index,
            excel_column,
            field,
            field_type: field_type.clone(),
            expand,
        });
    }

    for field_name in fields.keys() {
        if seen_fields.contains_key(field_name) {
            continue;
        }
        diagnostics.extend(table_load_error_diagnostics(
            TableLoadError::MissingColumn {
                location: Box::new(
                    TableLocation::new(source_name.to_path_buf())
                        .sheet(sheet.sheet.clone())
                        .with_row(header_excel_row),
                ),
                type_name: type_name.to_string(),
                field: field_name.clone(),
            },
        ));
    }

    let id_column = id_column.map(|(index, excel_column, _)| IdColumn {
        index,
        excel_column,
    });
    let Some(id_column) = id_column else {
        diagnostics.extend(table_load_error_diagnostics(
            TableLoadError::MissingKeyColumn {
                location: Box::new(
                    TableLocation::new(source_name.to_path_buf())
                        .sheet(sheet.sheet.clone())
                        .with_row(header_excel_row),
                ),
                type_name: type_name.to_string(),
                key: key_column,
            },
        ));
        return Err(TableDiagnostics { diagnostics });
    };

    if diagnostics.is_empty() {
        Ok(ResolvedColumns {
            columns,
            id_column,
            control_column,
        })
    } else {
        Err(TableDiagnostics { diagnostics })
    }
}

fn is_key_column(column_text: &str, field: &str, key_column: &str, has_explicit_key: bool) -> bool {
    if has_explicit_key {
        column_text == key_column
    } else {
        DEFAULT_KEY_COLUMN_ALIASES.contains(&column_text)
            || DEFAULT_KEY_COLUMN_ALIASES.contains(&field)
    }
}

#[allow(clippy::too_many_arguments)]
fn build_expanded_object(
    schema: &CftContainer,
    source_name: &Path,
    sheet: &TableSheetConfig,
    parent_type: &str,
    column: &ResolvedColumn,
    children: &[ExpandedSubColumn],
    row: &[String],
    excel_row: usize,
    diagnostics: &mut Vec<TableDiagnostic>,
) -> Option<CfdInputValue> {
    let mut fields = BTreeMap::new();
    let diagnostic_start = diagnostics.len();
    for child in children {
        let location = TableLocation::new(source_name.to_path_buf())
            .sheet(sheet.sheet.clone())
            .cell(excel_row, child.excel_column);
        let text = table_cell_text(row.get(child.index));
        let parsed = match parse_cell(schema, &child.field_type, &text) {
            Ok(parsed) => parsed,
            Err(err) => {
                diagnostics.extend(table_load_error_diagnostics(TableLoadError::CellParse {
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

fn path_column(path: &CfdPath, field_columns: &BTreeMap<Vec<String>, usize>) -> Option<usize> {
    let mut prefix = Vec::new();
    let mut column = None;
    for segment in &path.segments {
        let CfdPathSegment::Field(field) = segment else {
            break;
        };
        prefix.push(field.clone());
        if let Some(candidate) = field_columns.get(&prefix) {
            column = Some(*candidate);
        }
    }
    column
}

fn is_empty_mapped_row(row: &[String], columns: &[ResolvedColumn], id_column: &IdColumn) -> bool {
    row.get(id_column.index)
        .is_none_or(|cell| cell.trim().is_empty())
        && columns.iter().all(|column| {
            column.expand.as_ref().map_or_else(
                || {
                    row.get(column.index)
                        .is_none_or(|cell| cell.trim().is_empty())
                },
                |children| {
                    children.iter().all(|child| {
                        row.get(child.index)
                            .is_none_or(|cell| cell.trim().is_empty())
                    })
                },
            )
        })
}

fn should_skip_import_row(row: &[String], control_column: Option<usize>) -> bool {
    let Some(index) = control_column else {
        return false;
    };
    row.get(index)
        .is_some_and(|cell| cell.trim() == SKIP_IMPORT_ROW_MARKER)
}

fn table_cell_text(cell: Option<&String>) -> String {
    cell.cloned().unwrap_or_default()
}
