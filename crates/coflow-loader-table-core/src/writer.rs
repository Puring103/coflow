//! Shared table write planning for Excel-like data sources.
//!
//! Providers keep ownership of physical IO. This module turns source-neutral
//! writer requests into table mutations: set cells, append one row, or delete
//! one row. Excel, CSV, and remote sheet providers can apply those mutations
//! using their own storage/API layer.
mod cells;
mod diagnostics;

use coflow_data_model::{CfdDataModel, CfdPathSegment, CfdValue, RecordOrigin, SourceDocument};
use std::collections::BTreeMap;

use cells::{render_field_cells, render_insert_value};
use diagnostics::one_error;
pub use diagnostics::{TableWriteDiagnostic, TableWriteDiagnostics};

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
    Ok(TableWritePlan::AppendRow(TableAppendRow {
        document: request.document.clone(),
        sheet: request.sheet.to_string(),
        values,
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
