//! Writer that persists field edits back to local `.csv` files.
//!
//! `CsvWriter` is the [`SourceWriter`] for [`RecordOrigin::Table`] origins whose
//! document is `SourceDocument::Local` and whose backing file is a CSV. Each
//! call re-reads the file, applies the planned mutation in-memory, and writes
//! the whole document back. The CSV format has no sheet concept, so the plan's
//! `sheet` field is treated as a label and not validated against the file.

mod dimensions;
mod plan;
mod table_manager;

use coflow_api::{
    DeleteRecordRequest, Diagnostic, DiagnosticSet, InsertRecordRequest, RenameRecordRequest,
    RewriteRecordReferencesRequest, SourceLocationSpec, SourceWriter, WriteCellRequest,
    WriteContext, WriteOutcome, WriterCapabilities, WriterDescriptor,
};
use coflow_data_model::{CfdValue, SourceDocument};
use coflow_loader_table_core::writer::{
    plan_delete_record, plan_field_write, plan_insert_record, TableFieldWrite, TableInsertRecord,
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
