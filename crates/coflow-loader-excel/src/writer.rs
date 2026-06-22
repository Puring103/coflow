//! Writer that persists field edits back to local `.xlsx` workbooks.
//!
//! `ExcelWriter` is the [`DataWriter`] for [`RecordOrigin::Table`] origins
//! whose document is `SourceDocument::Local`. It uses
//! [`umya-spreadsheet`](https://docs.rs/umya-spreadsheet) so existing styles,
//! merged cells, and column widths survive round-trips.
use coflow_api::{
    CfdValue, DataWriter, Diagnostic, DiagnosticSet, RecordOrigin, SourceDocument,
    WriteCellRequest, WriteContext, WriteFieldPathSegment, WriteOutcome, WriterCapabilities,
    WriterDescriptor,
};
use std::path::Path;

pub const EXCEL_WRITER_DESCRIPTOR: WriterDescriptor = WriterDescriptor {
    id: "excel",
    display_name: "Excel workbook",
    capabilities: WriterCapabilities::local_full(),
};

/// Writer for local Excel workbooks.
///
/// Each call opens the file fresh — no in-memory cache, since
/// umya-spreadsheet load is fast for typical config workbooks and the disk is
/// always authoritative for external editors.
#[derive(Debug, Default)]
pub struct ExcelWriter;

impl ExcelWriter {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl DataWriter for ExcelWriter {
    fn descriptor(&self) -> &'static WriterDescriptor {
        &EXCEL_WRITER_DESCRIPTOR
    }

    fn write_field(
        &self,
        _ctx: WriteContext<'_>,
        request: &WriteCellRequest<'_>,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        let RecordOrigin::Table {
            document,
            sheet,
            row,
            id_column,
            field_columns,
        } = request.origin
        else {
            return Err(DiagnosticSet::one(diag(
                "EXCEL-WRITE",
                "excel writer requires a Table origin",
            )));
        };
        let SourceDocument::Local(path) = document else {
            return Err(DiagnosticSet::one(diag(
                "EXCEL-WRITE",
                "excel writer requires a local table document",
            )));
        };

        let column =
            resolve_column(request.field_path, field_columns, *id_column).ok_or_else(|| {
                DiagnosticSet::one(diag(
                    "EXCEL-WRITE",
                    format!(
                        "field path {:?} does not map to any column in the source row",
                        request.field_path
                    ),
                ))
            })?;
        let cell_value = render_cell_value(request.new_value)?;

        write_cell(path, sheet, *row, column, &cell_value)?;

        Ok(WriteOutcome {
            touched_record_origins: vec![request.origin.clone()],
            diagnostics: DiagnosticSet::empty(),
        })
    }
}

fn resolve_column(
    field_path: &[WriteFieldPathSegment],
    field_columns: &std::collections::BTreeMap<Vec<String>, usize>,
    id_column: usize,
) -> Option<usize> {
    if field_path.is_empty() {
        return Some(id_column);
    }
    // Build the longest prefix of Field segments and pick the deepest match
    // present in `field_columns`. Index/dict segments terminate the lookup.
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
    // If the path is the synthetic "id" field, fall back to the id column.
    if let Some(WriteFieldPathSegment::Field(name)) = field_path.first() {
        if name == "id" {
            return Some(id_column);
        }
    }
    None
}

/// Render a `CfdValue` into a textual cell payload. Matches the `cell_value`
/// parser's expectations on the read path so a round-trip preserves meaning.
fn render_cell_value(value: &CfdValue) -> Result<String, DiagnosticSet> {
    use std::fmt::Write;
    match value {
        CfdValue::Null => Ok(String::new()),
        CfdValue::Bool(v) => Ok(v.to_string()),
        CfdValue::Int(v) => Ok(v.to_string()),
        CfdValue::Float(v) => Ok(v.to_string()),
        CfdValue::String(v) => Ok(v.clone()),
        CfdValue::Enum(e) => Ok(e.variant.clone().unwrap_or_else(|| e.value.to_string())),
        CfdValue::Ref { key, .. } => Ok(format!("@{key}")),
        CfdValue::Array(items) => {
            let mut out = String::from("[");
            for (idx, item) in items.iter().enumerate() {
                if idx > 0 {
                    out.push_str(", ");
                }
                out.push_str(&render_cell_value(item)?);
            }
            out.push(']');
            Ok(out)
        }
        CfdValue::Dict(entries) => {
            let mut out = String::from("{");
            for (idx, (key, value)) in entries.iter().enumerate() {
                if idx > 0 {
                    out.push_str(", ");
                }
                let key_text = match key {
                    coflow_api::CfdDictKey::String(s) => format!("{s:?}"),
                    coflow_api::CfdDictKey::Int(n) => n.to_string(),
                    coflow_api::CfdDictKey::Enum(e) => {
                        e.variant.clone().unwrap_or_else(|| e.value.to_string())
                    }
                };
                let _ = write!(out, "{key_text}: {}", render_cell_value(value)?);
            }
            out.push('}');
            Ok(out)
        }
        CfdValue::Object(_) => Err(DiagnosticSet::one(diag(
            "EXCEL-WRITE",
            "writing nested object values into excel cells is not supported",
        ))),
    }
}

#[allow(clippy::cast_possible_truncation)]
fn write_cell(
    path: &Path,
    sheet: &str,
    row: usize,
    column: usize,
    value: &str,
) -> Result<(), DiagnosticSet> {
    if !path.exists() {
        return Err(DiagnosticSet::one(diag(
            "EXCEL-WRITE",
            format!("file `{}` does not exist", path.display()),
        )));
    }
    // Probe write access before doing the read+mutate work so a locked file
    // (Excel keeps an exclusive handle on the workbook it has open) fails
    // fast with a clear message instead of a generic "io error".
    if let Err(err) = probe_write_access(path) {
        return Err(DiagnosticSet::one(diag(
            "EXCEL-WRITE",
            humanize_io_error(path, &err, "open for writing"),
        )));
    }
    let mut book = umya_spreadsheet::reader::xlsx::read(path).map_err(|err| {
        DiagnosticSet::one(diag(
            "EXCEL-WRITE",
            format!("failed to read `{}`: {err:?}", path.display()),
        ))
    })?;
    let sheet_ref = book.get_sheet_by_name_mut(sheet).ok_or_else(|| {
        DiagnosticSet::one(diag(
            "EXCEL-WRITE",
            format!("sheet `{sheet}` not found in `{}`", path.display()),
        ))
    })?;
    sheet_ref
        .get_cell_mut((column as u32, row as u32))
        .set_value(value);
    umya_spreadsheet::writer::xlsx::write(&book, path).map_err(|err| {
        DiagnosticSet::one(diag(
            "EXCEL-WRITE",
            format!(
                "failed to save `{}`: {err:?}. \
                 If the workbook is open in Excel or another program, close it and retry.",
                path.display()
            ),
        ))
    })?;
    Ok(())
}

/// Open the file with read+write to surface OS-level "in use" / permission
/// errors as `std::io::Error`. The caller maps these to user-friendly
/// diagnostics. We deliberately drop the handle immediately — actually
/// performing the round-trip is umya-spreadsheet's job.
fn probe_write_access(path: &Path) -> std::io::Result<()> {
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .map(|_| ())
}

/// Translate a [`std::io::Error`] into a user-facing message that names the
/// likely cause (workbook open in Excel, no write permission, ...) instead
/// of leaking debug formatting.
fn humanize_io_error(path: &Path, err: &std::io::Error, action: &str) -> String {
    use std::io::ErrorKind;
    let display = path.display();
    let raw = err.raw_os_error().unwrap_or(0);
    // Windows: ERROR_SHARING_VIOLATION (32). The file is held by another
    // process — almost always Excel itself.
    if raw == 32 {
        return format!(
            "cannot {action} `{display}`: file is locked by another program (close Excel and retry)"
        );
    }
    match err.kind() {
        ErrorKind::PermissionDenied => format!(
            "cannot {action} `{display}`: permission denied (close any program holding the file or check file permissions)"
        ),
        ErrorKind::NotFound => format!("cannot {action} `{display}`: file does not exist"),
        _ => format!("cannot {action} `{display}`: {err}"),
    }
}

fn diag(code: &'static str, message: impl Into<String>) -> Diagnostic {
    Diagnostic::error(code, "EXCEL", message)
}
