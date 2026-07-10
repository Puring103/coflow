//! Excel-like table loader for Coflow data models.
//!
//! Source-specific loaders should convert their input into [`TableSource`]
//! values, then use this crate for schema-guided row, key, column, and cell
//! parsing.

mod columns;
mod diagnostics;
mod types;

use coflow_cft::{record_key_ident_error, CftContainer, CftSchemaView};
use coflow_data_model::{
    CfdDiagnostics, CfdInputRecord, CfdInputValue, CfdLabel, CfdPath, CfdPathSegment, RecordOrigin,
    SourceDocument,
};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::cell_value::{parse_cell, ParsedCell};
use columns::{
    field_columns_from_resolved, resolve_columns, ExpandedSubColumn, IdColumn, ResolvedColumn,
};
use diagnostics::{table_load_error_diagnostics, TableLoadError};
pub use types::{
    TableDiagnostic, TableDiagnostics, TableInputRecords, TableLabel, TableLocation, TableSheet,
    TableSheetConfig, TableSource, TableWriteLayout,
};

const SKIP_IMPORT_ROW_MARKER: &str = "##";

/// Loads table sources into input records without building a data model.
///
/// # Errors
///
/// Returns diagnostics when sheets, headers, keys, or cells cannot be loaded
/// according to the schema.
#[allow(clippy::too_many_lines)]
pub fn collect_table_input_records(
    schema: &CftSchemaView,
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

/// Resolves a table header into the field-column map used by table writers.
///
/// This uses the same schema-guided column resolution as the table loader,
/// including configured column aliases, key-column detection, and `@expand`
/// child columns.
///
/// # Errors
///
/// Returns diagnostics when the type is unknown, the header is missing required
/// columns, or `@expand` columns are malformed.
pub fn resolve_table_write_layout(
    schema: &CftContainer,
    source_name: &Path,
    sheet: &TableSheetConfig,
    header_row: &[String],
) -> Result<TableWriteLayout, TableDiagnostics> {
    let view = CftSchemaView::new(schema);
    let type_name = sheet.type_name();
    let Some(fields) = full_field_types(&view, type_name) else {
        return Err(TableDiagnostics {
            diagnostics: table_load_error_diagnostics(TableLoadError::UnknownType {
                location: Box::new(
                    TableLocation::new(source_name.to_path_buf()).sheet(sheet.sheet.clone()),
                ),
                type_name: type_name.to_string(),
            }),
        });
    };
    let resolved = resolve_columns(
        &view,
        source_name,
        sheet,
        type_name,
        &fields,
        header_row,
        1,
        1,
    )?;
    Ok(TableWriteLayout {
        id_column: resolved.id_column.excel_column,
        field_columns: field_columns_from_resolved(&resolved.columns),
    })
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
    RecordOrigin::Table {
        document,
        sheet,
        row,
        id_column,
        field_columns: field_columns_from_resolved(columns),
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

#[allow(clippy::too_many_arguments)]
fn build_expanded_object(
    schema: &CftSchemaView,
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

fn full_field_types(schema: &CftSchemaView, type_name: &str) -> Option<BTreeMap<String, String>> {
    let fields = schema
        .fields(type_name)?
        .map(|field| (field.name.clone(), field.raw_type.clone()))
        .collect();
    Some(fields)
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
