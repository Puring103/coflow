//! Read/write helpers for the synthetic "本地化" file tree.
//!
//! Localization CSVs are produced by the engine from `@localized` schema
//! fields, so they don't fit the regular `(source, model, record)` pipeline
//! used for authored data. Instead we surface them as a top-level virtual
//! folder and route reads/writes through the helpers in this module.
//!
//! Wire format: each CSV row becomes a `RecordRow` whose `key` is the row's
//! `id` column. Field cells are surfaced as plain string `FieldValue::Str`.
//! `id` and `default` are exposed read-only (the default value mirrors the
//! source record's `@localized` field; only translators edit lang columns).

use std::path::{Path, PathBuf};

use coflow_loader_csv::{parse, write};

use super::file_tree::LOCALIZATION_ROOT;
use crate::editor::types::{
    EditorError, FieldCell, FieldValue, FileRecords, RecordRow, SourceCapabilities,
};

/// Is this front-end file path a synthetic localization entry?
#[must_use]
pub(super) fn is_localization_path(file_path: &str) -> bool {
    file_path == LOCALIZATION_ROOT || file_path.starts_with(&format!("{LOCALIZATION_ROOT}/"))
}

/// The static `FieldCell.name` we use for the read-only id and default
/// columns, in their canonical column order. Lang columns appear after
/// these, in the order they're declared in `localization.languages`.
const ID_FIELD: &str = "id";
const DEFAULT_FIELD: &str = "default";

/// Reserved schema field names for the synthetic record. The front-end
/// uses these strings to decide which cells are editable.
pub(super) const LOCALIZATION_TYPE: &str = "LocalizationEntry";

#[derive(Debug)]
pub(super) struct LocalizationFile {
    /// Absolute on-disk path of the CSV.
    pub disk_path: PathBuf,
    /// Header row preserved verbatim so writes can update an existing
    /// column without reordering the file.
    pub header: Vec<String>,
    /// Data rows (header excluded).
    pub rows: Vec<Vec<String>>,
}

/// Resolve a `__localization__/foo.csv` style virtual path to an absolute
/// CSV path under `project_root/<localization.out_dir>`.
fn resolve_csv_path(
    project_root: &Path,
    localization_out_dir: &Path,
    file_path: &str,
) -> Result<PathBuf, EditorError> {
    let Some(rel) = file_path.strip_prefix(&format!("{LOCALIZATION_ROOT}/")) else {
        return Err(EditorError::not_found(format!(
            "`{file_path}` is not a localization file"
        )));
    };
    if rel.is_empty() || rel.contains('/') || rel.contains('\\') {
        return Err(EditorError::not_found(format!(
            "invalid localization path `{file_path}`"
        )));
    }
    let dir = if localization_out_dir.is_absolute() {
        localization_out_dir.to_path_buf()
    } else {
        project_root.join(localization_out_dir)
    };
    Ok(dir.join(rel))
}

/// Read and parse a localization CSV.
pub(super) fn load(
    project_root: &Path,
    localization_out_dir: &Path,
    file_path: &str,
) -> Result<LocalizationFile, EditorError> {
    let disk_path = resolve_csv_path(project_root, localization_out_dir, file_path)?;
    let text = std::fs::read_to_string(&disk_path).map_err(|err| {
        EditorError::other(format!(
            "failed to read localization file `{}`: {err}",
            disk_path.display()
        ))
    })?;
    let mut parsed = parse(&text).map_err(|err| {
        EditorError::other(format!(
            "failed to parse localization file `{}`: {err}",
            disk_path.display()
        ))
    })?;
    let header = if parsed.is_empty() {
        Vec::new()
    } else {
        parsed.remove(0)
    };
    Ok(LocalizationFile {
        disk_path,
        header,
        rows: parsed,
    })
}

/// Render a parsed localization CSV as a `FileRecords` so the editor's
/// standard TableView can display it. The id and default columns become
/// `read_only` fields; remaining language columns are editable.
pub(super) fn to_file_records(file_path: &str, csv: &LocalizationFile) -> FileRecords {
    let header = &csv.header;
    let id_col = header.iter().position(|h| h == ID_FIELD).unwrap_or(0);
    let default_col = header.iter().position(|h| h == DEFAULT_FIELD);

    let mut records: Vec<RecordRow> = Vec::with_capacity(csv.rows.len());
    for row in &csv.rows {
        let key = row.get(id_col).cloned().unwrap_or_default();
        if key.is_empty() {
            continue;
        }
        let mut fields: Vec<FieldCell> = Vec::with_capacity(header.len());
        for (col, name) in header.iter().enumerate() {
            if col == id_col {
                continue; // key is rendered separately
            }
            let value = row.get(col).cloned().unwrap_or_default();
            fields.push(FieldCell {
                name: name.clone(),
                value: FieldValue::Str { v: value },
                is_spread: false,
                spread_info: None,
                read_only: default_col == Some(col),
            });
        }
        records.push(RecordRow {
            key,
            actual_type: LOCALIZATION_TYPE.to_string(),
            fields,
        });
    }

    FileRecords {
        file_path: file_path.to_string(),
        type_names: vec![LOCALIZATION_TYPE.to_string()],
        records,
        capabilities: SourceCapabilities::localization(),
    }
}

/// Apply a single-cell edit to a localization CSV, then write the result
/// back to disk. Only language columns are writeable; attempts to edit
/// `id` or `default` are rejected.
pub(super) fn write_field(
    project_root: &Path,
    localization_out_dir: &Path,
    file_path: &str,
    record_key: &str,
    field_name: &str,
    new_value: &str,
) -> Result<LocalizationFile, EditorError> {
    if field_name == ID_FIELD || field_name == DEFAULT_FIELD {
        return Err(EditorError::write(format!(
            "localization field `{field_name}` is read-only"
        )));
    }
    let mut csv = load(project_root, localization_out_dir, file_path)?;
    let id_col = csv
        .header
        .iter()
        .position(|h| h == ID_FIELD)
        .ok_or_else(|| EditorError::write("localization CSV is missing `id` column"))?;
    let target_col = csv
        .header
        .iter()
        .position(|h| h == field_name)
        .ok_or_else(|| {
            EditorError::write(format!(
                "localization column `{field_name}` not found in CSV"
            ))
        })?;
    let row = csv
        .rows
        .iter_mut()
        .find(|row| row.get(id_col).map(String::as_str) == Some(record_key))
        .ok_or_else(|| {
            EditorError::not_found(format!(
                "localization row `{record_key}` not found in `{file_path}`"
            ))
        })?;
    while row.len() <= target_col {
        row.push(String::new());
    }
    row[target_col] = new_value.to_string();

    let mut all_rows: Vec<Vec<String>> = Vec::with_capacity(csv.rows.len() + 1);
    all_rows.push(csv.header.clone());
    all_rows.extend(csv.rows.iter().cloned());
    let body = write(&all_rows);
    std::fs::write(&csv.disk_path, body).map_err(|err| {
        EditorError::write(format!(
            "failed to write localization file `{}`: {err}",
            csv.disk_path.display()
        ))
    })?;
    Ok(csv)
}
