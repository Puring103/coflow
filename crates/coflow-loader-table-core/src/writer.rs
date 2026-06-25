//! Shared table write planning for Excel-like data sources.
//!
//! Providers keep ownership of physical IO. This module turns source-neutral
//! writer requests into table mutations: set cells, append one row, or delete
//! one row. Excel, CSV, and remote sheet providers can apply those mutations
//! using their own storage/API layer.
use coflow_data_model::{CfdDataModel, CfdValue, RecordOrigin, SourceDocument};
use std::collections::BTreeMap;

use crate::cell_value::{render_cell_value, CellRenderError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteFieldPathSegment {
    Field(String),
    Index(usize),
}

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

fn render_insert_value(
    record_key: &str,
    actual_type: &str,
    path: &[WriteFieldPathSegment],
    value: &CfdValue,
    field_columns: &BTreeMap<Vec<String>, usize>,
) -> Result<Vec<(usize, String)>, TableWriteDiagnostics> {
    if let CfdValue::Object(record) = value {
        let prefix = field_prefix(path)?;
        let child_columns = direct_child_columns(field_columns, &prefix);
        if child_columns.is_empty() {
            let column = resolve_column(path, field_columns, 0)
                .ok_or_else(|| unmapped_path_error(record_key, actual_type, path))?;
            let value = render_cell_value(value).map_err(table_render_error)?;
            return Ok(vec![(column, value)]);
        }
        let mut cells = Vec::new();
        for (field, child_value) in &record.fields {
            let Some(column) = child_columns.get(field) else {
                return Err(unmapped_path_error(record_key, actual_type, path));
            };
            let value = render_cell_value(child_value).map_err(table_render_error)?;
            cells.push((*column, value));
        }
        return Ok(cells);
    }
    let column = resolve_column(path, field_columns, 0)
        .ok_or_else(|| unmapped_path_error(record_key, actual_type, path))?;
    let value = render_cell_value(value).map_err(table_render_error)?;
    Ok(vec![(column, value)])
}

#[allow(clippy::too_many_arguments)]
fn render_field_cells(
    record_key: &str,
    actual_type: &str,
    path: &[WriteFieldPathSegment],
    new_value: &CfdValue,
    model: Option<&CfdDataModel>,
    row: usize,
    field_columns: &BTreeMap<Vec<String>, usize>,
    id_column: usize,
) -> Result<Vec<TableSetCell>, TableWriteDiagnostics> {
    if is_id_path(path) {
        let value = render_cell_value(new_value).map_err(table_render_error)?;
        return Ok(vec![TableSetCell {
            row,
            column: id_column,
            value,
        }]);
    }
    if let CfdValue::Object(record) = new_value {
        let prefix = field_prefix(path)?;
        let child_columns = direct_child_columns(field_columns, &prefix);
        if !child_columns.is_empty() {
            let mut cells = Vec::new();
            for (field, value) in &record.fields {
                let Some(column) = child_columns.get(field) else {
                    return Err(unmapped_path_error(record_key, actual_type, path));
                };
                cells.push(TableSetCell {
                    row,
                    column: *column,
                    value: render_cell_value(value).map_err(table_render_error)?,
                });
            }
            return Ok(cells);
        }
    }
    let column_path = column_path(path);
    let column = resolve_column(&column_path, field_columns, id_column)
        .ok_or_else(|| unmapped_path_error(record_key, actual_type, path))?;
    let cell_value = if path == column_path.as_slice() {
        new_value.clone()
    } else {
        let model = model.ok_or_else(|| {
            one_error(
                "TABLE-WRITE",
                "editing inside a table collection cell requires the current data model",
            )
        })?;
        let root_value = root_value_for_path(model, actual_type, record_key, &column_path)?;
        replace_subvalue(root_value, &path[column_path.len()..], new_value.clone())?
    };
    Ok(vec![TableSetCell {
        row,
        column,
        value: render_cell_value(&cell_value).map_err(table_render_error)?,
    }])
}

fn root_value_for_path(
    model: &CfdDataModel,
    actual_type: &str,
    record_key: &str,
    path: &[WriteFieldPathSegment],
) -> Result<CfdValue, TableWriteDiagnostics> {
    let record = model
        .lookup(actual_type, record_key)
        .and_then(|id| model.record(id))
        .ok_or_else(|| {
            one_error(
                "TABLE-WRITE",
                format!("record `{actual_type}.{record_key}` not found in current model"),
            )
        })?;
    let Some(WriteFieldPathSegment::Field(field)) = path.first() else {
        return Err(one_error(
            "TABLE-WRITE",
            "table field path must start with a field",
        ));
    };
    record
        .field(field)
        .cloned()
        .ok_or_else(|| one_error("TABLE-WRITE", format!("field `{field}` not found")))
}

fn replace_subvalue(
    mut value: CfdValue,
    path: &[WriteFieldPathSegment],
    new_value: CfdValue,
) -> Result<CfdValue, TableWriteDiagnostics> {
    if path.is_empty() {
        return Ok(new_value);
    }
    match (&path[0], &mut value) {
        (WriteFieldPathSegment::Index(index), CfdValue::Array(items)) => {
            let item = items
                .get_mut(*index)
                .ok_or_else(|| one_error("TABLE-WRITE", format!("index {index} out of bounds")))?;
            *item = replace_subvalue(item.clone(), &path[1..], new_value)?;
            Ok(value)
        }
        (WriteFieldPathSegment::Field(field), CfdValue::Object(record)) => {
            let current =
                record.fields.get(field).cloned().ok_or_else(|| {
                    one_error("TABLE-WRITE", format!("field `{field}` not found"))
                })?;
            record.fields.insert(
                field.clone(),
                replace_subvalue(current, &path[1..], new_value)?,
            );
            Ok(value)
        }
        _ => Err(one_error(
            "TABLE-WRITE",
            format!("cannot navigate path segment {:?}", path[0]),
        )),
    }
}

fn resolve_column(
    field_path: &[WriteFieldPathSegment],
    field_columns: &BTreeMap<Vec<String>, usize>,
    id_column: usize,
) -> Option<usize> {
    if field_path.is_empty() {
        return Some(id_column);
    }
    let mut prefix: Vec<String> = Vec::new();
    let mut found = None;
    for segment in field_path {
        let WriteFieldPathSegment::Field(name) = segment else {
            break;
        };
        prefix.push(name.clone());
        if let Some(column) = field_columns.get(&prefix) {
            found = Some(*column);
        }
    }
    if found.is_some() {
        return found;
    }
    if is_id_path(field_path) {
        return Some(id_column);
    }
    None
}

fn column_path(path: &[WriteFieldPathSegment]) -> Vec<WriteFieldPathSegment> {
    let mut out = Vec::new();
    for segment in path {
        match segment {
            WriteFieldPathSegment::Field(field) => {
                out.push(WriteFieldPathSegment::Field(field.clone()));
            }
            WriteFieldPathSegment::Index(_) => break,
        }
    }
    out
}

fn field_prefix(path: &[WriteFieldPathSegment]) -> Result<Vec<String>, TableWriteDiagnostics> {
    let mut out = Vec::new();
    for segment in path {
        let WriteFieldPathSegment::Field(field) = segment else {
            return Err(one_error(
                "TABLE-WRITE",
                "expanded table field path must contain only field segments",
            ));
        };
        out.push(field.clone());
    }
    Ok(out)
}

fn direct_child_columns(
    field_columns: &BTreeMap<Vec<String>, usize>,
    prefix: &[String],
) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    for (path, column) in field_columns {
        if path.len() == prefix.len() + 1 && path.starts_with(prefix) {
            out.insert(path[prefix.len()].clone(), *column);
        }
    }
    out
}

fn is_id_path(path: &[WriteFieldPathSegment]) -> bool {
    matches!(path, [WriteFieldPathSegment::Field(name)] if name == "id")
}

fn unmapped_path_error(
    record_key: &str,
    actual_type: &str,
    path: &[WriteFieldPathSegment],
) -> TableWriteDiagnostics {
    one_error(
        "TABLE-WRITE",
        format!(
            "field path {path:?} on record `{actual_type}.{record_key}` does not map to any table column"
        ),
    )
}

fn table_render_error(err: CellRenderError) -> TableWriteDiagnostics {
    let message = match err {
        CellRenderError::AnonymousEnum => {
            "writing anonymous enum values into table cells is not supported"
        }
        CellRenderError::NestedObject => {
            "writing nested object values into table cells is not supported"
        }
    };
    one_error("TABLE-WRITE", message)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableWriteDiagnostics {
    pub diagnostics: Vec<TableWriteDiagnostic>,
}

impl TableWriteDiagnostics {
    pub fn iter(&self) -> std::slice::Iter<'_, TableWriteDiagnostic> {
        self.diagnostics.iter()
    }
}

impl<'a> IntoIterator for &'a TableWriteDiagnostics {
    type Item = &'a TableWriteDiagnostic;
    type IntoIter = std::slice::Iter<'a, TableWriteDiagnostic>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableWriteDiagnostic {
    pub code: String,
    pub stage: String,
    pub message: String,
}

fn one_error(code: &'static str, message: impl Into<String>) -> TableWriteDiagnostics {
    TableWriteDiagnostics {
        diagnostics: vec![TableWriteDiagnostic {
            code: code.to_string(),
            stage: "TABLE".to_string(),
            message: message.into(),
        }],
    }
}
