//! Writer that persists field edits back to local `.xlsx` workbooks.
//!
//! `ExcelWriter` is the [`SourceWriter`] for [`RecordOrigin::Table`] origins
//! whose document is `SourceDocument::Local`. It uses
//! [`umya-spreadsheet`](https://docs.rs/umya-spreadsheet) so existing styles,
//! merged cells, and column widths survive round-trips.
mod format;
mod table_manager;

use crate::options::{
    excel_sheet_config_from_options, excel_sheet_for_type_from_options, excel_source_options,
    ExcelSourceOptions,
};
use calamine::Reader;
use coflow_api::{
    DeleteRecordRequest, Diagnostic, DiagnosticSet, InsertRecordRequest, RenameRecordRequest,
    RewriteRecordReferencesRequest, SourceLocationSpec, SourceWriter, WriteBatchFailure,
    WriteCellRequest, WriteContext, WriteOutcome, WriterCapabilities, WriterDescriptor,
};
use coflow_data_model::{CfdValue, SourceDocument};
use coflow_loader_table_core::writer::{
    plan_delete_record, plan_field_write, plan_insert_record, TableAppendRow, TableDeleteRow,
    TableFieldWrite, TableInsertRecord, TableSetCell, TableWriteDiagnostics, TableWritePlan,
};
use coflow_loader_table_core::{resolve_table_write_layout, TableDiagnostics};
use format::{ensure_writable_excel_path, excel_writer_capabilities};
use std::collections::BTreeMap;
use std::path::Path;

pub static EXCEL_WRITER_DESCRIPTOR: WriterDescriptor = WriterDescriptor {
    id: "excel",
    display_name: "Excel workbook",
    capabilities: WriterCapabilities {
        provider_id: String::new(),
        can_edit_field: true,
        can_edit_key: true,
        can_insert_record: true,
        can_delete_record: true,
        requires_full_refresh_after_write: true,
    },
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

impl SourceWriter for ExcelWriter {
    fn descriptor(&self) -> &'static WriterDescriptor {
        &EXCEL_WRITER_DESCRIPTOR
    }

    fn capabilities(&self, source: &coflow_api::ResolvedSource) -> WriterCapabilities {
        excel_writer_capabilities(source)
    }

    fn preflight(&self, _ctx: WriteContext<'_>, request: &WriteCellRequest<'_>) -> DiagnosticSet {
        let SourceLocationSpec::Path(path) = &request.source.location;
        ensure_writable_excel_path(path, "edit fields")
            .err()
            .unwrap_or_default()
    }

    fn write_field(
        &self,
        ctx: WriteContext<'_>,
        request: &WriteCellRequest<'_>,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        let SourceLocationSpec::Path(path) = &request.source.location;
        ensure_writable_excel_path(path, "edit fields")?;
        let plan = plan_field_write(&TableFieldWrite {
            origin: request.origin,
            record_key: request.record_key,
            actual_type: request.actual_type,
            field_path: request.field_path,
            new_value: request.new_value,
            model: ctx.model,
        })
        .map_err(table_write_diagnostics_to_api)?;
        apply_plan(&plan)?;
        Ok(WriteOutcome::default())
    }

    fn write_field_batch(
        &self,
        ctx: WriteContext<'_>,
        requests: &[WriteCellRequest<'_>],
    ) -> Result<Vec<WriteOutcome>, WriteBatchFailure> {
        let mut plans = Vec::with_capacity(requests.len());
        for (index, request) in requests.iter().enumerate() {
            let SourceLocationSpec::Path(path) = &request.source.location;
            ensure_writable_excel_path(path, "edit fields")
                .map_err(|diagnostics| WriteBatchFailure { index, diagnostics })?;
            let plan = plan_field_write(&TableFieldWrite {
                origin: request.origin,
                record_key: request.record_key,
                actual_type: request.actual_type,
                field_path: request.field_path,
                new_value: request.new_value,
                model: ctx.model,
            })
            .map_err(table_write_diagnostics_to_api)
            .map_err(|diagnostics| WriteBatchFailure { index, diagnostics })?;
            plans.push(plan);
        }
        apply_plans(&plans)
            .map_err(|(index, diagnostics)| WriteBatchFailure { index, diagnostics })?;
        Ok(vec![WriteOutcome::default(); requests.len()])
    }

    fn insert_record(
        &self,
        _ctx: WriteContext<'_>,
        request: &InsertRecordRequest<'_>,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        let SourceLocationSpec::Path(path) = &request.source.location;
        ensure_writable_excel_path(path, "insert records")?;
        let sheet = match request.sheet {
            Some(sheet) => sheet.to_string(),
            None => excel_sheet_for_type_from_options(
                excel_source_options(request.source)?,
                request.actual_type,
            )?
            .unwrap_or_else(|| request.actual_type.to_string()),
        };
        let layout = read_sheet_layout(
            path,
            &sheet,
            request.actual_type,
            excel_source_options(request.source)?,
            request.schema,
        )?;
        let plan = plan_insert_record(&TableInsertRecord {
            document: SourceDocument::Local(path.clone()),
            sheet: &sheet,
            record_key: request.record_key,
            actual_type: request.actual_type,
            fields: request.fields,
            field_columns: &layout.field_columns,
            id_column: layout.id_column,
        })
        .map_err(table_write_diagnostics_to_api)?;
        apply_plan(&plan)?;
        Ok(WriteOutcome::default())
    }

    fn rename_record(
        &self,
        ctx: WriteContext<'_>,
        request: &RenameRecordRequest<'_>,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        let path = [coflow_api::WriteFieldPathSegment::Field("id".to_string())];
        let value = CfdValue::String(request.new_key.to_string());
        self.write_field(
            ctx,
            &WriteCellRequest {
                origin: request.origin,
                record_key: request.old_key,
                actual_type: request.actual_type,
                field_path: &path,
                new_value: &value,
                schema: request.schema,
                source: request.source,
            },
        )
    }

    fn delete_record(
        &self,
        _ctx: WriteContext<'_>,
        request: &DeleteRecordRequest<'_>,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        let SourceLocationSpec::Path(path) = &request.source.location;
        ensure_writable_excel_path(path, "delete records")?;
        let plan = plan_delete_record(request.origin, request.record_key)
            .map_err(table_write_diagnostics_to_api)?;
        apply_plan(&plan)?;
        Ok(WriteOutcome::default())
    }

    fn rewrite_record_references(
        &self,
        _ctx: WriteContext<'_>,
        _request: &RewriteRecordReferencesRequest<'_>,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        Ok(WriteOutcome::default())
    }
}

fn apply_plan(plan: &TableWritePlan) -> Result<(), DiagnosticSet> {
    apply_plans(std::slice::from_ref(plan)).map_err(|(_, diagnostics)| diagnostics)
}

fn apply_plans(plans: &[TableWritePlan]) -> Result<(), (usize, DiagnosticSet)> {
    let Some(first) = plans.first() else {
        return Ok(());
    };
    let path = local_plan_path(first);
    for (index, plan) in plans.iter().enumerate().skip(1) {
        let candidate = local_plan_path(plan);
        if candidate != path {
            return Err((
                index,
                DiagnosticSet::one(diag(
                    "EXCEL-WRITE",
                    "excel field batch spans more than one workbook",
                )),
            ));
        }
    }
    let mut failed_index = 0;
    mutate_workbook(path, |book| {
        for (index, plan) in plans.iter().enumerate() {
            failed_index = index;
            apply_plan_to_workbook(book, path, plan)?;
        }
        Ok(())
    })
    .map_err(|diagnostics| (failed_index, diagnostics))
}

fn local_plan_path(plan: &TableWritePlan) -> &Path {
    match plan {
        TableWritePlan::SetCells { document, .. }
        | TableWritePlan::AppendRow(TableAppendRow { document, .. })
        | TableWritePlan::DeleteRow(TableDeleteRow { document, .. }) => {
            let SourceDocument::Local(path) = document;
            path
        }
    }
}

fn apply_plan_to_workbook(
    book: &mut umya_spreadsheet::Spreadsheet,
    path: &Path,
    plan: &TableWritePlan,
) -> Result<(), DiagnosticSet> {
    match plan {
        TableWritePlan::SetCells {
            sheet,
            id_column,
            expected_key,
            cells,
            ..
        } => {
            let sheet_ref = mutable_sheet(book, path, sheet)?;
            let Some(first) = cells.first() else {
                return Ok(());
            };
            ensure_expected_key(sheet_ref, path, sheet, first.row, *id_column, expected_key)?;
            for cell in cells {
                write_sheet_cell(sheet_ref, cell)?;
            }
            Ok(())
        }
        TableWritePlan::AppendRow(TableAppendRow { sheet, values, .. }) => {
            let sheet_ref = mutable_sheet(book, path, sheet)?;
            let row = excel_usize(sheet_ref.get_highest_row(), "row")? + 1;
            for (column, value) in values {
                let coord = excel_coord(*column, row)?;
                sheet_ref.get_cell_mut(coord).set_value(value);
            }
            Ok(())
        }
        TableWritePlan::DeleteRow(TableDeleteRow {
            sheet,
            row,
            id_column,
            expected_key,
            ..
        }) => {
            let sheet_ref = mutable_sheet(book, path, sheet)?;
            ensure_expected_key(sheet_ref, path, sheet, *row, *id_column, expected_key)?;
            let row = excel_index(*row, "row")?;
            sheet_ref.remove_row(&row, &1);
            Ok(())
        }
    }
}

fn mutate_workbook(
    path: &Path,
    mutate: impl FnOnce(&mut umya_spreadsheet::Spreadsheet) -> Result<(), DiagnosticSet>,
) -> Result<(), DiagnosticSet> {
    ensure_writable_excel_path(path, "mutate workbook")?;
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
    mutate(&mut book)?;
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

fn mutable_sheet<'a>(
    book: &'a mut umya_spreadsheet::Spreadsheet,
    path: &Path,
    sheet: &str,
) -> Result<&'a mut umya_spreadsheet::Worksheet, DiagnosticSet> {
    // Resolve the requested name against the workbook's actual sheet
    // names, allowing a trimmed / whitespace-tolerant fallback when the
    // exact name doesn't match. We've seen real workbooks where calamine
    // surfaced a sheet name with hidden whitespace (full-width space, BOM,
    // zero-width joiners) that umya stored without — strict equality then
    // misses the sheet.
    let target = normalize_sheet_name(sheet);
    let names: Vec<String> = book
        .get_sheet_collection_no_check()
        .iter()
        .map(|s| s.get_name().to_string())
        .collect();
    let resolved = names
        .iter()
        .find(|name| name.as_str() == sheet || normalize_sheet_name(name) == target)
        .cloned();

    if let Some(name) = resolved {
        if let Some(ws) = book.get_sheet_by_name_mut(&name) {
            return Ok(ws);
        }
    }

    // Surface the candidate list so users can see whether it's a typo or
    // a hidden whitespace / encoding issue.
    let available = names
        .iter()
        .map(|name| format!("`{name}`"))
        .collect::<Vec<_>>()
        .join(", ");
    let available = if available.is_empty() {
        "(workbook has no sheets)".to_string()
    } else {
        format!("available: {available}")
    };
    Err(DiagnosticSet::one(diag(
        "EXCEL-WRITE",
        format!(
            "sheet `{sheet}` not found in `{}` ({available})",
            path.display()
        ),
    )))
}

/// Normalize a sheet name for tolerant comparison: trim outer whitespace
/// (including the full-width space `U+3000` and zero-width joiners that
/// sometimes leak in via copy-paste) and strip BOM / zero-width
/// formatting characters. We deliberately do **not** lowercase — Excel
/// sheet names are case-sensitive on the wire.
///
/// Not a full Unicode NFC normalize: we don't pull in the
/// `unicode-normalization` crate just for this edge case. If a workbook
/// ever surfaces decomposed CJK marks that mismatch umya's stored form,
/// we'll revisit.
fn normalize_sheet_name(name: &str) -> String {
    name.trim_matches(|c: char| c.is_whitespace() || is_invisible_format_char(c))
        .chars()
        .filter(|c| !is_invisible_format_char(*c))
        .collect()
}

const fn is_invisible_format_char(c: char) -> bool {
    matches!(
        c,
        '\u{200B}'  // ZERO WIDTH SPACE
        | '\u{200C}' // ZERO WIDTH NON-JOINER
        | '\u{200D}' // ZERO WIDTH JOINER
        | '\u{FEFF}' // ZERO WIDTH NO-BREAK SPACE / BOM
        | '\u{00A0}' // NO-BREAK SPACE
        | '\u{3000}' // IDEOGRAPHIC SPACE
    )
}

fn write_sheet_cell(
    sheet: &mut umya_spreadsheet::Worksheet,
    cell: &TableSetCell,
) -> Result<(), DiagnosticSet> {
    let coord = excel_coord(cell.column, cell.row)?;
    sheet.get_cell_mut(coord).set_value(&cell.value);
    Ok(())
}

fn ensure_expected_key(
    sheet: &umya_spreadsheet::Worksheet,
    path: &Path,
    sheet_name: &str,
    row: usize,
    id_column: usize,
    expected_key: &str,
) -> Result<(), DiagnosticSet> {
    let coord = excel_coord(id_column, row)?;
    let actual = sheet
        .get_cell(coord)
        .map_or_else(String::new, |cell| cell.get_value().to_string());
    if actual.trim() == expected_key {
        return Ok(());
    }
    Err(DiagnosticSet::one(diag(
        "EXCEL-WRITE",
        format!(
            "row {row} in `{}` sheet `{sheet_name}` expected key `{expected_key}` but found `{}`",
            path.display(),
            actual.trim()
        ),
    )))
}

fn excel_coord(column: usize, row: usize) -> Result<(u32, u32), DiagnosticSet> {
    Ok((excel_index(column, "column")?, excel_index(row, "row")?))
}

fn excel_index(value: usize, label: &str) -> Result<u32, DiagnosticSet> {
    if value == 0 {
        return Err(DiagnosticSet::one(diag(
            "EXCEL-WRITE",
            format!("excel {label} index must be at least 1"),
        )));
    }
    u32::try_from(value).map_err(|_| {
        DiagnosticSet::one(diag(
            "EXCEL-WRITE",
            format!("excel {label} index {value} is out of range"),
        ))
    })
}

fn excel_usize(value: u32, label: &str) -> Result<usize, DiagnosticSet> {
    usize::try_from(value).map_err(|_| {
        DiagnosticSet::one(diag(
            "EXCEL-WRITE",
            format!("excel {label} index {value} is out of range"),
        ))
    })
}

#[derive(Debug)]
struct SheetLayout {
    id_column: usize,
    field_columns: BTreeMap<Vec<String>, usize>,
}

fn read_sheet_layout(
    path: &Path,
    sheet: &str,
    actual_type: &str,
    options: &ExcelSourceOptions,
    schema: &coflow_cft::CftSchema,
) -> Result<SheetLayout, DiagnosticSet> {
    let config = excel_sheet_config_from_options(options, sheet, actual_type)?;
    let mut workbook = calamine::open_workbook_auto(path).map_err(|err| {
        DiagnosticSet::one(diag(
            "EXCEL-WRITE",
            format!("failed to read `{}`: {err}", path.display()),
        ))
    })?;
    let range = workbook.worksheet_range(sheet).map_err(|err| {
        DiagnosticSet::one(diag(
            "EXCEL-WRITE",
            format!("sheet `{sheet}` not found in `{}`: {err}", path.display()),
        ))
    })?;
    let Some(header) = range.rows().next() else {
        return Err(DiagnosticSet::one(diag(
            "EXCEL-WRITE",
            format!("sheet `{sheet}` is empty"),
        )));
    };
    let header = header
        .iter()
        .map(excel_cell_to_text)
        .collect::<Vec<String>>();
    let layout = resolve_table_write_layout(schema, path, &config, &header)
        .map_err(table_diagnostics_to_api)?;
    Ok(SheetLayout {
        id_column: layout.id_column,
        field_columns: layout.field_columns,
    })
}

fn excel_cell_to_text(cell: &calamine::Data) -> String {
    match cell {
        calamine::Data::Empty => String::new(),
        calamine::Data::String(value) => value.clone(),
        calamine::Data::Float(value) if value.fract() == 0.0 => format!("{value:.0}"),
        calamine::Data::Float(value) => value.to_string(),
        calamine::Data::Int(value) => value.to_string(),
        calamine::Data::Bool(value) => value.to_string(),
        other => format!("{other}"),
    }
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

fn table_write_diagnostics_to_api(err: TableWriteDiagnostics) -> DiagnosticSet {
    err.diagnostics
        .into_iter()
        .map(|diagnostic| diag("EXCEL-WRITE", diagnostic.message))
        .collect::<Vec<_>>()
        .into()
}

fn table_diagnostics_to_api(err: TableDiagnostics) -> DiagnosticSet {
    err.diagnostics
        .into_iter()
        .map(|diagnostic| diag("EXCEL-WRITE", diagnostic.message))
        .collect::<Vec<_>>()
        .into()
}
