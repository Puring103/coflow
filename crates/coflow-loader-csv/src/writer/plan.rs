use coflow_api::{DiagnosticSet, RecordOrigin, SourceDocument};
use coflow_loader_table_core::writer::{
    TableAppendRow, TableDeleteRow, TableSetCell, TableWritePlan,
};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use super::diag;
use crate::{parse, write};

pub(super) fn apply_plan(plan: &TableWritePlan) -> Result<Option<RecordOrigin>, DiagnosticSet> {
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
                let idx = row.checked_sub(1).ok_or_else(|| {
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
    let row_idx = cell
        .row
        .checked_sub(1)
        .ok_or_else(|| DiagnosticSet::one(diag("CSV-WRITE", "csv row index must be at least 1")))?;
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
    row[col_idx].clone_from(&cell.value);
    Ok(())
}

fn ensure_expected_key(
    rows: &[Vec<String>],
    path: &Path,
    row: usize,
    id_column: usize,
    expected_key: &str,
) -> Result<(), DiagnosticSet> {
    let row_idx = row
        .checked_sub(1)
        .ok_or_else(|| DiagnosticSet::one(diag("CSV-WRITE", "csv row index must be at least 1")))?;
    let col_idx = id_column.checked_sub(1).ok_or_else(|| {
        DiagnosticSet::one(diag("CSV-WRITE", "csv column index must be at least 1"))
    })?;
    let actual = rows
        .get(row_idx)
        .and_then(|r| r.get(col_idx))
        .map_or("", String::as_str);
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
