//! Writer that persists field edits back to local `.csv` files.
//!
//! `CsvWriter` is the [`DataWriter`] for [`RecordOrigin::Table`] origins whose
//! document is `SourceDocument::Local` and whose backing file is a CSV. Each
//! call re-reads the file, applies the planned mutation in-memory, and writes
//! the whole document back. The CSV format has no sheet concept, so the plan's
//! `sheet` field is treated as a label and not validated against the file.

use coflow_api::{
    DataWriter, DeleteRecordRequest, Diagnostic, DiagnosticSet, InsertRecordRequest, RecordOrigin,
    SourceDocument, SourceLocationSpec, WriteCellRequest, WriteContext, WriteOutcome,
    WriterCapabilities, WriterDescriptor,
};
use coflow_loader_table_core::writer::{
    plan_delete_record, plan_field_write, plan_insert_record, TableAppendRow, TableDeleteRow,
    TableFieldWrite, TableInsertRecord, TableSetCell, TableWriteDiagnostics, TableWritePlan,
    WriteFieldPathSegment as TableWriteFieldPathSegment,
};
use coflow_loader_table_core::{resolve_table_write_layout, TableDiagnostics, TableSheetConfig};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::{parse, write};

pub const CSV_WRITER_DESCRIPTOR: WriterDescriptor = WriterDescriptor {
    id: "csv",
    display_name: "CSV file",
    capabilities: WriterCapabilities::local_full(),
};

/// Writer for local CSV files. Stateless: each call reads the file fresh and
/// writes back, so external edits are picked up automatically.
#[derive(Debug, Default)]
pub struct CsvWriter;

impl CsvWriter {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl DataWriter for CsvWriter {
    fn descriptor(&self) -> &'static WriterDescriptor {
        &CSV_WRITER_DESCRIPTOR
    }

    fn write_field(
        &self,
        ctx: WriteContext<'_>,
        request: &WriteCellRequest<'_>,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        let plan = plan_field_write(&TableFieldWrite {
            origin: request.origin,
            record_key: request.record_key,
            actual_type: request.actual_type,
            field_path: &request
                .field_path
                .iter()
                .map(api_path_segment_to_table)
                .collect::<Vec<_>>(),
            new_value: request.new_value,
            model: ctx.model,
        })
        .map_err(table_write_diagnostics_to_api)?;
        apply_plan(&plan)?;
        Ok(WriteOutcome {
            touched_record_origins: vec![request.origin.clone()],
            inserted_record_origin: None,
            deleted_record_origin: None,
            diagnostics: DiagnosticSet::empty(),
        })
    }

    fn insert_record(
        &self,
        _ctx: WriteContext<'_>,
        request: &InsertRecordRequest<'_>,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        let SourceLocationSpec::Path(path) = &request.source.location else {
            return Err(DiagnosticSet::one(diag(
                "CSV-WRITE",
                "csv writer requires a local path source",
            )));
        };
        // CSV has exactly one sheet (the file itself). If the caller picked
        // a sheet name, accept it as a label; otherwise fall back to the
        // file stem so the resulting plan's `sheet` field is non-empty.
        let sheet = request.sheet.unwrap_or_else(|| default_sheet_name(path));
        let layout = read_csv_layout(
            path,
            sheet,
            request.actual_type,
            &request.source.options,
            request.schema,
        )?;
        let plan = plan_insert_record(&TableInsertRecord {
            document: SourceDocument::Local(path.clone()),
            sheet,
            record_key: request.record_key,
            actual_type: request.actual_type,
            fields: request.fields,
            field_columns: &layout.field_columns,
            id_column: layout.id_column,
        })
        .map_err(table_write_diagnostics_to_api)?;
        let inserted_origin = apply_plan(&plan)?;
        Ok(WriteOutcome {
            touched_record_origins: Vec::new(),
            inserted_record_origin: inserted_origin,
            deleted_record_origin: None,
            diagnostics: DiagnosticSet::empty(),
        })
    }

    fn delete_record(
        &self,
        _ctx: WriteContext<'_>,
        request: &DeleteRecordRequest<'_>,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        let plan = plan_delete_record(request.origin, request.record_key)
            .map_err(table_write_diagnostics_to_api)?;
        apply_plan(&plan)?;
        Ok(WriteOutcome {
            touched_record_origins: Vec::new(),
            inserted_record_origin: None,
            deleted_record_origin: Some(request.origin.clone()),
            diagnostics: DiagnosticSet::empty(),
        })
    }
}

fn apply_plan(plan: &TableWritePlan) -> Result<Option<RecordOrigin>, DiagnosticSet> {
    match plan {
        TableWritePlan::SetCells {
            document,
            sheet: _,
            id_column,
            expected_key,
            cells,
        } => {
            let path = local_path(document)?;
            mutate_csv(path, |rows| {
                let Some(first) = cells.first() else {
                    return Ok(None);
                };
                ensure_expected_key(rows, path, first.row, *id_column, expected_key)?;
                for cell in cells {
                    set_csv_cell(rows, cell)?;
                }
                Ok(None)
            })
        }
        TableWritePlan::AppendRow(TableAppendRow {
            document,
            sheet,
            values,
        }) => {
            let path = local_path(document)?;
            let sheet = sheet.clone();
            mutate_csv(path, |rows| {
                // 1-based row index of the new row.
                let row = rows.len() + 1;
                let id_column = values.iter().map(|(column, _)| *column).min().unwrap_or(1);
                let mut field_columns = BTreeMap::new();
                for (column, value) in values {
                    set_csv_cell(
                        rows,
                        &TableSetCell {
                            row,
                            column: *column,
                            value: value.clone(),
                        },
                    )?;
                    if *column != id_column {
                        field_columns.insert(vec![format!("column_{column}")], *column);
                    }
                }
                Ok(Some(RecordOrigin::Table {
                    document: SourceDocument::Local(path.to_path_buf()),
                    sheet,
                    row,
                    id_column,
                    field_columns,
                }))
            })
        }
        TableWritePlan::DeleteRow(TableDeleteRow {
            document,
            sheet: _,
            row,
            id_column,
            expected_key,
        }) => {
            let path = local_path(document)?;
            mutate_csv(path, |rows| {
                ensure_expected_key(rows, path, *row, *id_column, expected_key)?;
                let idx = row
                    .checked_sub(1)
                    .ok_or_else(|| {
                        DiagnosticSet::one(diag(
                            "CSV-WRITE",
                            "csv row index must be at least 1".to_string(),
                        ))
                    })?;
                if idx < rows.len() {
                    rows.remove(idx);
                }
                Ok(None)
            })
        }
    }
}

fn local_path(document: &SourceDocument) -> Result<&Path, DiagnosticSet> {
    let SourceDocument::Local(path) = document else {
        return Err(DiagnosticSet::one(diag(
            "CSV-WRITE",
            "csv writer requires a local table document",
        )));
    };
    Ok(path)
}

/// Read the CSV, hand the mutable rows to `mutate`, then write the result
/// back. Adding columns to a row that is shorter than the target column is
/// supported (empty cells are inserted as needed) — Excel-like layout
/// resolution may locate the id column past the existing width.
fn mutate_csv(
    path: &Path,
    mutate: impl FnOnce(&mut Vec<Vec<String>>) -> Result<Option<RecordOrigin>, DiagnosticSet>,
) -> Result<Option<RecordOrigin>, DiagnosticSet> {
    if !path.exists() {
        return Err(DiagnosticSet::one(diag(
            "CSV-WRITE",
            format!("file `{}` does not exist", path.display()),
        )));
    }
    let text = fs::read_to_string(path).map_err(|err| {
        DiagnosticSet::one(diag(
            "CSV-WRITE",
            format!("failed to read `{}`: {err}", path.display()),
        ))
    })?;
    let mut rows = parse(&text).map_err(|err| {
        DiagnosticSet::one(diag(
            "CSV-WRITE",
            format!("failed to parse `{}`: {err}", path.display()),
        ))
    })?;
    let origin = mutate(&mut rows)?;
    let body = write(&rows);
    fs::write(path, body).map_err(|err| {
        DiagnosticSet::one(diag(
            "CSV-WRITE",
            format!("failed to write `{}`: {err}", path.display()),
        ))
    })?;
    Ok(origin)
}

fn set_csv_cell(rows: &mut Vec<Vec<String>>, cell: &TableSetCell) -> Result<(), DiagnosticSet> {
    let row_idx = cell.row.checked_sub(1).ok_or_else(|| {
        DiagnosticSet::one(diag("CSV-WRITE", "csv row index must be at least 1"))
    })?;
    let col_idx = cell.column.checked_sub(1).ok_or_else(|| {
        DiagnosticSet::one(diag("CSV-WRITE", "csv column index must be at least 1"))
    })?;
    while rows.len() <= row_idx {
        rows.push(Vec::new());
    }
    let row = &mut rows[row_idx];
    while row.len() <= col_idx {
        row.push(String::new());
    }
    row[col_idx] = cell.value.clone();
    Ok(())
}

fn ensure_expected_key(
    rows: &[Vec<String>],
    path: &Path,
    row: usize,
    id_column: usize,
    expected_key: &str,
) -> Result<(), DiagnosticSet> {
    let row_idx = row.checked_sub(1).ok_or_else(|| {
        DiagnosticSet::one(diag("CSV-WRITE", "csv row index must be at least 1"))
    })?;
    let col_idx = id_column.checked_sub(1).ok_or_else(|| {
        DiagnosticSet::one(diag("CSV-WRITE", "csv column index must be at least 1"))
    })?;
    let actual = rows
        .get(row_idx)
        .and_then(|r| r.get(col_idx))
        .map(String::as_str)
        .unwrap_or("");
    if actual.trim() == expected_key {
        return Ok(());
    }
    Err(DiagnosticSet::one(diag(
        "CSV-WRITE",
        format!(
            "row {row} in `{}` expected key `{expected_key}` but found `{}`",
            path.display(),
            actual.trim()
        ),
    )))
}

struct CsvLayout {
    id_column: usize,
    field_columns: BTreeMap<Vec<String>, usize>,
}

fn read_csv_layout(
    path: &Path,
    sheet: &str,
    actual_type: &str,
    options: &Value,
    schema: &coflow_cft::CftContainer,
) -> Result<CsvLayout, DiagnosticSet> {
    let text = fs::read_to_string(path).map_err(|err| {
        DiagnosticSet::one(diag(
            "CSV-WRITE",
            format!("failed to read `{}`: {err}", path.display()),
        ))
    })?;
    let rows = parse(&text).map_err(|err| {
        DiagnosticSet::one(diag(
            "CSV-WRITE",
            format!("failed to parse `{}`: {err}", path.display()),
        ))
    })?;
    let Some(header) = rows.first() else {
        return Err(DiagnosticSet::one(diag(
            "CSV-WRITE",
            format!("csv file `{}` is empty", path.display()),
        )));
    };
    let config = sheet_config_from_options(options, sheet, actual_type);
    let layout = resolve_table_write_layout(schema, path, &config, header)
        .map_err(table_diagnostics_to_api)?;
    Ok(CsvLayout {
        id_column: layout.id_column,
        field_columns: layout.field_columns,
    })
}

fn sheet_config_from_options(options: &Value, sheet: &str, actual_type: &str) -> TableSheetConfig {
    let Some(sheets) = options.get("sheets").and_then(Value::as_array) else {
        return TableSheetConfig::new(sheet).with_type(actual_type);
    };
    for item in sheets {
        let Some(object) = item.as_object() else {
            continue;
        };
        let matches_sheet = object
            .get("sheet")
            .and_then(Value::as_str)
            .is_some_and(|candidate| candidate == sheet);
        let matches_type = object
            .get("type")
            .and_then(Value::as_str)
            .is_some_and(|candidate| candidate == actual_type);
        if !matches_sheet && !matches_type {
            continue;
        }
        let mut config = TableSheetConfig::new(sheet).with_type(
            object
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or(actual_type),
        );
        if let Some(key) = object.get("key").and_then(Value::as_str) {
            config = config.with_key(key);
        }
        let columns = object
            .get("columns")
            .and_then(Value::as_object)
            .map_or_else(BTreeMap::new, |columns| {
                columns
                    .iter()
                    .filter_map(|(source, field)| {
                        field
                            .as_str()
                            .map(|field| (source.clone(), field.to_string()))
                    })
                    .collect()
            });
        if !columns.is_empty() {
            config = config.with_columns(columns);
        }
        return config;
    }
    TableSheetConfig::new(sheet).with_type(actual_type)
}

fn default_sheet_name(path: &Path) -> &str {
    path.file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("csv")
}

fn diag(code: &'static str, message: impl Into<String>) -> Diagnostic {
    Diagnostic::error(code, "CSV", message)
}

fn api_path_segment_to_table(
    segment: &coflow_api::WriteFieldPathSegment,
) -> TableWriteFieldPathSegment {
    match segment {
        coflow_api::WriteFieldPathSegment::Field(field) => {
            TableWriteFieldPathSegment::Field(field.clone())
        }
        coflow_api::WriteFieldPathSegment::Index(index) => {
            TableWriteFieldPathSegment::Index(*index)
        }
    }
}

fn table_write_diagnostics_to_api(err: TableWriteDiagnostics) -> DiagnosticSet {
    err.diagnostics
        .into_iter()
        .map(|diagnostic| diag("CSV-WRITE", diagnostic.message))
        .collect::<Vec<_>>()
        .into()
}

fn table_diagnostics_to_api(err: TableDiagnostics) -> DiagnosticSet {
    err.diagnostics
        .into_iter()
        .map(|diagnostic| diag("CSV-WRITE", diagnostic.message))
        .collect::<Vec<_>>()
        .into()
}
