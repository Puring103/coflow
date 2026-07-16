use coflow_data_model::{format_cfd_dict_key, CfdDataModel, CfdValue};
use std::collections::BTreeMap;

use crate::cell_value::render_cell_value;

use super::diagnostics::{one_error, table_render_error};
use super::{TableSetCell, TableWriteDiagnostics, WriteFieldPathSegment};

pub(super) fn render_insert_value(
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
        for (field, child_value) in record.fields() {
            let Some(column) = child_columns.get(field.as_str()) else {
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
pub(super) fn render_field_cells(
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
    if is_field_only_path(path) {
        if let CfdValue::Object(record) = new_value {
            let prefix = field_prefix(path)?;
            let child_columns = direct_child_columns(field_columns, &prefix);
            if !child_columns.is_empty() {
                let mut cells = Vec::new();
                for (field, value) in record.fields() {
                    let Some(column) = child_columns.get(field.as_str()) else {
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
        .record_by_type_key(actual_type, record_key)
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
            let (field_name, current) = record
                .fields()
                .get_key_value(field.as_str())
                .map(|(name, value)| (name.clone(), value.clone()))
                .ok_or_else(|| one_error("TABLE-WRITE", format!("field `{field}` not found")))?;
            record.fields.insert(
                field_name,
                replace_subvalue(current, &path[1..], new_value)?,
            );
            Ok(value)
        }
        (WriteFieldPathSegment::DictKey(key), CfdValue::Dict(entries)) => {
            let index = entries
                .iter()
                .position(|(entry_key, _)| format_cfd_dict_key(entry_key) == *key)
                .ok_or_else(|| one_error("TABLE-WRITE", format!("dict key `{key}` not found")))?;
            let current = entries[index].1.clone();
            entries[index].1 = replace_subvalue(current, &path[1..], new_value)?;
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
            WriteFieldPathSegment::Index(_) | WriteFieldPathSegment::DictKey(_) => break,
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

fn is_field_only_path(path: &[WriteFieldPathSegment]) -> bool {
    path.iter()
        .all(|segment| matches!(segment, WriteFieldPathSegment::Field(_)))
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
