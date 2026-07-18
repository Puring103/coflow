//! Writer that persists field edits back to local `.csv` files.
//!
//! `CsvWriter` is the [`SourceWriter`] for [`coflow_data_model::RecordOrigin::Table`] origins whose
//! document is `SourceDocument::Local` and whose backing file is a CSV. Each
//! call re-reads the file, applies the planned mutation in-memory, and writes
//! the whole document back. The CSV format has no sheet concept, so the plan's
//! `sheet` field is treated as a label and not validated against the file.

mod dimensions;
mod plan;
mod table_manager;

use coflow_api::{
    DeleteRecordRequest, Diagnostic, DiagnosticSet, InsertRecordRequest, RenameRecordRequest,
    ReorderRecordsOperation, ReorderRecordsRequest, RewriteRecordReferencesRequest,
    SourceLocationSpec, SourceWriter, WriteCellRequest, WriteContext, WriteOutcome,
    WriterCapabilities, WriterDescriptor,
};
use coflow_data_model::{CfdValue, RecordOrigin, SourceDocument};
use coflow_loader_table_core::writer::{
    plan_delete_record, plan_field_write, plan_insert_record, plan_reorder_records,
    TableFieldWrite, TableInsertRecord, TableRecordRef, TableReorderOperation,
    TableWriteDiagnostics,
};
use coflow_loader_table_core::{resolve_table_write_layout, TableDiagnostics};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::options::{csv_sheet_config_from_options, csv_source_options, CsvSourceOptions};
use crate::parse;
use plan::apply_plan;

pub static CSV_WRITER_DESCRIPTOR: WriterDescriptor = WriterDescriptor {
    id: "csv",
    display_name: "CSV file",
    capabilities: WriterCapabilities {
        provider_id: String::new(),
        can_edit_field: true,
        can_edit_key: true,
        can_insert_record: true,
        can_delete_record: true,
        can_reorder_records: true,
        requires_full_refresh_after_write: true,
    },
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

impl SourceWriter for CsvWriter {
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
            field_path: request.field_path,
            new_value: request.new_value,
            model: ctx.model,
        })
        .map_err(table_write_diagnostics_to_api)?;
        apply_plan(&plan)?;
        Ok(WriteOutcome::default())
    }

    fn insert_record(
        &self,
        _ctx: WriteContext<'_>,
        request: &InsertRecordRequest<'_>,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        let SourceLocationSpec::Path(path) = &request.source.location;
        // CSV has exactly one sheet (the file itself). If the caller picked
        // a sheet name, accept it as a label; otherwise fall back to the
        // file stem so the resulting plan's `sheet` field is non-empty.
        let sheet = request.sheet.unwrap_or_else(|| default_sheet_name(path));
        let layout = read_csv_layout(
            path,
            sheet,
            request.actual_type,
            csv_source_options(request.source)?,
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
            before: request.before.map(|before| TableRecordRef {
                origin: before.origin,
                record_key: before.record_key,
            }),
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
        ensure_table_origin_path(request.origin, path)?;
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

    fn reorder_records(
        &self,
        _ctx: WriteContext<'_>,
        request: &ReorderRecordsRequest<'_>,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        let SourceLocationSpec::Path(path) = &request.source.location;
        let operation = match request.operation {
            ReorderRecordsOperation::Swap { first, second } => {
                if first.actual_type != second.actual_type {
                    return Err(DiagnosticSet::one(diag(
                        "CSV-WRITE",
                        "records must have the same type to exchange positions",
                    )));
                }
                ensure_table_origin_path(first.origin, path)?;
                ensure_table_origin_path(second.origin, path)?;
                TableReorderOperation::Swap {
                    first: TableRecordRef {
                        origin: first.origin,
                        record_key: first.record_key,
                    },
                    second: TableRecordRef {
                        origin: second.origin,
                        record_key: second.record_key,
                    },
                }
            }
            ReorderRecordsOperation::MoveBefore { record, before } => {
                ensure_table_origin_path(record.origin, path)?;
                if let Some(before) = before {
                    ensure_table_origin_path(before.origin, path)?;
                }
                TableReorderOperation::MoveBefore {
                    record: TableRecordRef {
                        origin: record.origin,
                        record_key: record.record_key,
                    },
                    before: before.map(|before| TableRecordRef {
                        origin: before.origin,
                        record_key: before.record_key,
                    }),
                }
            }
        };
        let plan = plan_reorder_records(operation).map_err(table_write_diagnostics_to_api)?;
        apply_plan(&plan)?;
        Ok(WriteOutcome::default())
    }
}

fn ensure_table_origin_path(origin: &RecordOrigin, expected: &Path) -> Result<(), DiagnosticSet> {
    match origin {
        RecordOrigin::Table {
            document: SourceDocument::Local(path),
            ..
        } if path == expected => Ok(()),
        RecordOrigin::Table {
            document: SourceDocument::Local(path),
            ..
        } => Err(DiagnosticSet::one(diag(
            "CSV-WRITE",
            format!(
                "record origin `{}` does not match source `{}`",
                path.display(),
                expected.display()
            ),
        ))),
        _ => Err(DiagnosticSet::one(diag(
            "CSV-WRITE",
            "csv write requires a local table origin",
        ))),
    }
}

struct CsvLayout {
    id_column: usize,
    field_columns: BTreeMap<Vec<String>, usize>,
}

fn read_csv_layout(
    path: &Path,
    sheet: &str,
    actual_type: &str,
    options: &CsvSourceOptions,
    schema: &coflow_cft::CftSchema,
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
    let config = csv_sheet_config_from_options(options, sheet, actual_type)?;
    let layout = resolve_table_write_layout(schema, path, &config, header)
        .map_err(table_diagnostics_to_api)?;
    Ok(CsvLayout {
        id_column: layout.id_column,
        field_columns: layout.field_columns,
    })
}

fn default_sheet_name(path: &Path) -> &str {
    path.file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("csv")
}

pub(super) fn diag(code: &'static str, message: impl Into<String>) -> Diagnostic {
    Diagnostic::error(code, "CSV", message)
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
