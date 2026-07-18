//! Shared table write planning for Excel-like data sources.
//!
//! Providers keep ownership of physical IO. This module turns source-neutral
//! writer requests into table mutations: set cells, append one row, or delete
//! one row. Excel, CSV, and remote sheet providers can apply those mutations
//! using their own storage/API layer.
mod cells;
mod diagnostics;
mod header;

use coflow_data_model::{CfdDataModel, CfdPathSegment, CfdValue, RecordOrigin, SourceDocument};
use std::collections::BTreeMap;

use cells::{render_field_cells, render_insert_value};
use diagnostics::one_error;
pub use diagnostics::{TableWriteDiagnostic, TableWriteDiagnostics};
pub use header::HeaderReconciliationPlan;

pub type WriteFieldPathSegment = CfdPathSegment;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableSetCell {
    pub row: usize,
    pub column: usize,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableAppendRow {
    pub document: SourceDocument,
    pub sheet: String,
    pub values: Vec<(usize, String)>,
    pub before_row: Option<usize>,
    pub before_id_column: Option<usize>,
    pub expected_before_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableDeleteRow {
    pub document: SourceDocument,
    pub sheet: String,
    pub row: usize,
    pub id_column: usize,
    pub expected_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableSwapRows {
    pub document: SourceDocument,
    pub sheet: String,
    pub first_row: usize,
    pub first_id_column: usize,
    pub expected_first_key: String,
    pub second_row: usize,
    pub second_id_column: usize,
    pub expected_second_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableMoveRowBefore {
    pub document: SourceDocument,
    pub sheet: String,
    pub row: usize,
    pub id_column: usize,
    pub expected_key: String,
    pub before_row: Option<usize>,
    pub before_id_column: Option<usize>,
    pub expected_before_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TableWritePlan {
    SetCells {
        document: SourceDocument,
        sheet: String,
        id_column: usize,
        expected_key: String,
        cells: Vec<TableSetCell>,
    },
    AppendRow(TableAppendRow),
    DeleteRow(TableDeleteRow),
    SwapRows(TableSwapRows),
    MoveRowBefore(TableMoveRowBefore),
}

#[derive(Debug, Clone)]
pub struct TableInsertRecord<'a> {
    pub document: SourceDocument,
    pub sheet: &'a str,
    pub record_key: &'a str,
    pub actual_type: &'a str,
    pub fields: &'a BTreeMap<String, CfdValue>,
    pub field_columns: &'a BTreeMap<Vec<String>, usize>,
    pub id_column: usize,
    pub before: Option<TableRecordRef<'a>>,
}

#[derive(Debug, Clone)]
pub struct TableFieldWrite<'a> {
    pub origin: &'a RecordOrigin,
    pub record_key: &'a str,
    pub actual_type: &'a str,
    pub field_path: &'a [WriteFieldPathSegment],
    pub new_value: &'a CfdValue,
    pub model: Option<&'a CfdDataModel>,
}

#[derive(Debug, Clone, Copy)]
pub struct TableRecordRef<'a> {
    pub origin: &'a RecordOrigin,
    pub record_key: &'a str,
}

#[derive(Debug, Clone, Copy)]
pub enum TableReorderOperation<'a> {
    Swap {
        first: TableRecordRef<'a>,
        second: TableRecordRef<'a>,
    },
    MoveBefore {
        record: TableRecordRef<'a>,
        before: Option<TableRecordRef<'a>>,
    },
}

/// Build a table mutation plan for a field edit.
///
/// # Errors
///
/// Returns diagnostics when the origin is not a table row, the field path
/// cannot be mapped to existing columns, or the value cannot be rendered as
/// table-cell text.
pub fn plan_field_write(
    request: &TableFieldWrite<'_>,
) -> Result<TableWritePlan, TableWriteDiagnostics> {
    let RecordOrigin::Table {
        document,
        sheet,
        row,
        id_column,
        field_columns,
    } = request.origin
    else {
        return Err(one_error(
            "TABLE-WRITE",
            "table writer requires a Table origin",
        ));
    };
    let cells = render_field_cells(
        request.record_key,
        request.actual_type,
        request.field_path,
        request.new_value,
        request.model,
        *row,
        field_columns,
        *id_column,
    )?;
    Ok(TableWritePlan::SetCells {
        document: document.clone(),
        sheet: sheet.clone(),
        id_column: *id_column,
        expected_key: request.record_key.to_string(),
        cells,
    })
}

/// Build a table mutation plan for appending a new record row.
///
/// # Errors
///
/// Returns diagnostics when one of the requested fields cannot be mapped to an
/// existing column or a value cannot be rendered as table-cell text.
pub fn plan_insert_record(
    request: &TableInsertRecord<'_>,
) -> Result<TableWritePlan, TableWriteDiagnostics> {
    let mut values = vec![(request.id_column, request.record_key.to_string())];
    for (field, value) in request.fields {
        let path = [WriteFieldPathSegment::Field(field.clone())];
        let rendered = render_insert_value(
            request.record_key,
            request.actual_type,
            &path,
            value,
            request.field_columns,
        )?;
        values.extend(rendered);
    }
    values.sort_by_key(|(column, _)| *column);
    values.dedup_by_key(|(column, _)| *column);
    let before = request.before.map(table_position).transpose()?;
    if let Some(before) = &before {
        if before.document != &request.document || before.sheet != request.sheet {
            return Err(one_error(
                "TABLE-WRITE",
                "insert anchor must belong to the target table document and sheet",
            ));
        }
    }
    Ok(TableWritePlan::AppendRow(TableAppendRow {
        document: request.document.clone(),
        sheet: request.sheet.to_string(),
        values,
        before_row: before.as_ref().map(|position| position.row),
        before_id_column: before.as_ref().map(|position| position.id_column),
        expected_before_key: before
            .as_ref()
            .map(|position| position.expected_key.to_string()),
    }))
}

/// Build a table mutation plan for deleting one record row.
///
/// # Errors
///
/// Returns diagnostics when the origin is not a table row.
pub fn plan_delete_record(
    origin: &RecordOrigin,
    expected_key: &str,
) -> Result<TableWritePlan, TableWriteDiagnostics> {
    let RecordOrigin::Table {
        document,
        sheet,
        row,
        id_column,
        ..
    } = origin
    else {
        return Err(one_error(
            "TABLE-WRITE",
            "table writer requires a Table origin",
        ));
    };
    Ok(TableWritePlan::DeleteRow(TableDeleteRow {
        document: document.clone(),
        sheet: sheet.clone(),
        row: *row,
        id_column: *id_column,
        expected_key: expected_key.to_string(),
    }))
}

/// Build one atomic table row reorder plan.
///
/// # Errors
///
/// Returns diagnostics when any record is not table-backed or the records do
/// not belong to the same document and sheet.
pub fn plan_reorder_records(
    operation: TableReorderOperation<'_>,
) -> Result<TableWritePlan, TableWriteDiagnostics> {
    match operation {
        TableReorderOperation::Swap { first, second } => {
            let first = table_position(first)?;
            let second = table_position(second)?;
            ensure_same_container(&first, &second)?;
            Ok(TableWritePlan::SwapRows(TableSwapRows {
                document: first.document.clone(),
                sheet: first.sheet.to_string(),
                first_row: first.row,
                first_id_column: first.id_column,
                expected_first_key: first.expected_key.to_string(),
                second_row: second.row,
                second_id_column: second.id_column,
                expected_second_key: second.expected_key.to_string(),
            }))
        }
        TableReorderOperation::MoveBefore { record, before } => {
            let record = table_position(record)?;
            let before = before.map(table_position).transpose()?;
            if let Some(before) = &before {
                ensure_same_container(&record, before)?;
            }
            Ok(TableWritePlan::MoveRowBefore(TableMoveRowBefore {
                document: record.document.clone(),
                sheet: record.sheet.to_string(),
                row: record.row,
                id_column: record.id_column,
                expected_key: record.expected_key.to_string(),
                before_row: before.as_ref().map(|position| position.row),
                before_id_column: before.as_ref().map(|position| position.id_column),
                expected_before_key: before
                    .as_ref()
                    .map(|position| position.expected_key.to_string()),
            }))
        }
    }
}

struct TablePosition<'a> {
    document: &'a SourceDocument,
    sheet: &'a str,
    row: usize,
    id_column: usize,
    expected_key: &'a str,
}

fn table_position(record: TableRecordRef<'_>) -> Result<TablePosition<'_>, TableWriteDiagnostics> {
    let RecordOrigin::Table {
        document,
        sheet,
        row,
        id_column,
        ..
    } = record.origin
    else {
        return Err(one_error(
            "TABLE-WRITE",
            "table reorder requires a Table origin",
        ));
    };
    Ok(TablePosition {
        document,
        sheet,
        row: *row,
        id_column: *id_column,
        expected_key: record.record_key,
    })
}

fn ensure_same_container(
    left: &TablePosition<'_>,
    right: &TablePosition<'_>,
) -> Result<(), TableWriteDiagnostics> {
    if left.document == right.document && left.sheet == right.sheet {
        return Ok(());
    }
    Err(one_error(
        "TABLE-WRITE",
        "records must belong to the same table document and sheet",
    ))
}
