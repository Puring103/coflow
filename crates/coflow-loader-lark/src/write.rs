use coflow_api::{
    CreateTableRequest, DeleteRecordRequest, DiagnosticSet, InsertRecordRequest,
    RenameRecordRequest, RewriteRecordReferencesRequest, SourceLocationSpec, SourceWriter,
    SyncHeaderRequest, TableAddressing, TableContext, TableHeaderOptions, TableManager,
    TableManagerDescriptor, TableOperationResult, WriteCellRequest, WriteContext,
    WriteFieldPathSegment, WriteOutcome, WriterCapabilities, WriterDescriptor,
};
use coflow_data_model::{CfdValue, RecordOrigin, SourceDocument};
use coflow_loader_table_core::writer::{
    plan_field_write, plan_insert_record, HeaderReconciliationPlan, TableFieldWrite,
    TableInsertRecord, TableWritePlan,
};
use coflow_loader_table_core::TableSheetConfig;
use serde_json::json;

use crate::diagnostics::{diag, table_write_diagnostics_to_api};
use crate::http::LarkHttpClient;
use crate::source::{
    lark_document_spreadsheet_token, lark_source_options, sheet_config_from_options,
    sheet_for_type_from_options, type_for_sheet_from_options,
};
use crate::write_http::LarkWriteFailure;
use crate::write_layout::{lark_insert_layout, LarkInsertLayoutRequest};
use crate::{column_name, url_component, LarkSheetWriter, API_BASE};

/// Writer descriptor for Lark sheets.
pub static LARK_SHEET_WRITER_DESCRIPTOR: WriterDescriptor = WriterDescriptor {
    id: "lark-sheet",
    display_name: "Lark Sheet",
    capabilities: WriterCapabilities {
        provider_id: String::new(),
        can_edit_field: true,
        can_edit_key: true,
        can_insert_record: true,
        can_delete_record: true,
        requires_full_refresh_after_write: true,
        is_remote: true,
    },
};

pub static LARK_SHEET_TABLE_MANAGER_DESCRIPTOR: TableManagerDescriptor = TableManagerDescriptor {
    id: "lark-sheet",
    display_name: "Lark Sheet",
    file_extensions: &[],
    aliases: &[],
    addressing: TableAddressing::Sheet,
};

impl<C> SourceWriter for LarkSheetWriter<C>
where
    C: LarkHttpClient + Send + Sync,
{
    fn descriptor(&self) -> &'static WriterDescriptor {
        &LARK_SHEET_WRITER_DESCRIPTOR
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
        let TableWritePlan::SetCells {
            document,
            sheet,
            cells,
            ..
        } = plan
        else {
            return Err(DiagnosticSet::one(diag(
                "LARK-WRITE",
                "lark field writes must produce table cell updates",
            )));
        };
        let SourceDocument::Remote(doc) = &document else {
            return Err(DiagnosticSet::one(diag(
                "LARK-WRITE",
                "lark writer requires a remote table document",
            )));
        };

        let options = lark_source_options(request.source)?;
        let token = self.cached_tenant_token(&options.app_id, &options.app_secret)?;
        let spreadsheet_token = self.lark_spreadsheet_token_from_source(request.source, &token)?;
        let same_source_uri =
            matches!(&request.source.location, SourceLocationSpec::Uri(uri) if uri == doc);
        let same_spreadsheet =
            lark_document_spreadsheet_token(doc).as_deref() == Some(spreadsheet_token.as_str());
        if !same_source_uri && !same_spreadsheet {
            return Err(DiagnosticSet::one(diag(
                "LARK-WRITE",
                "record origin does not belong to the requested lark source",
            )));
        }

        let sheet_id = self.cached_sheet_id(&spreadsheet_token, &sheet, &token)?;
        let value_ranges = cells
            .iter()
            .map(|cell| {
                let column_letters = column_name(cell.column);
                let range = format!(
                    "{sheet_id}!{column_letters}{}:{column_letters}{}",
                    cell.row, cell.row
                );
                json!({ "range": range, "values": [[cell.value.clone()]] })
            })
            .collect::<Vec<_>>();
        let endpoint = format!(
            "{API_BASE}/sheets/v2/spreadsheets/{}/values_batch_update",
            url_component(&spreadsheet_token)
        );
        let body = json!({
            "valueRanges": value_ranges
        });
        let outcome = match self.send_values_batch_update(&endpoint, &body, &token) {
            Ok(()) => Ok(()),
            Err(LarkWriteFailure::TokenExpired(diag_set)) => {
                self.invalidate_caches(Some(&options.app_id), None);
                let fresh = self.cached_tenant_token(&options.app_id, &options.app_secret)?;
                self.send_values_batch_update(&endpoint, &body, &fresh)
                    .map_err(|err| match err {
                        LarkWriteFailure::TokenExpired(d) | LarkWriteFailure::Other(d) => d,
                    })
                    .map_err(|d| {
                        let mut combined = diag_set.clone();
                        combined.extend(d);
                        combined
                    })
            }
            Err(LarkWriteFailure::Other(diag_set)) => Err(diag_set),
        };
        outcome?;

        Ok(WriteOutcome {
            touched_record_origins: vec![request.origin.clone()],
            inserted_record_origin: None,
            deleted_record_origin: None,
            diagnostics: DiagnosticSet::empty(),
        })
    }

    fn rename_record(
        &self,
        ctx: WriteContext<'_>,
        request: &RenameRecordRequest<'_>,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        let path = [WriteFieldPathSegment::Field("id".to_string())];
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

    fn insert_record(
        &self,
        ctx: WriteContext<'_>,
        request: &InsertRecordRequest<'_>,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        let auth = self.lark_write_auth(request.source)?;
        let spreadsheet_token =
            self.lark_spreadsheet_token_from_source(request.source, &auth.token)?;
        let sheet = match request.sheet {
            Some(sheet) => sheet.to_string(),
            None => sheet_for_type_from_options(
                lark_source_options(request.source)?,
                request.actual_type,
            )?
                .unwrap_or_else(|| request.actual_type.to_string()),
        };
        let sheet_id = self.cached_sheet_id(&spreadsheet_token, &sheet, &auth.token)?;
        let layout = lark_insert_layout(&LarkInsertLayoutRequest {
            ctx,
            writer: self,
            source: request.source,
            spreadsheet_token: &spreadsheet_token,
            sheet_id: &sheet_id,
            sheet: &sheet,
            actual_type: request.actual_type,
            token: &auth.token,
        })?;
        let plan = plan_insert_record(&TableInsertRecord {
            document: SourceDocument::Remote(format!("lark:{spreadsheet_token}")),
            sheet: &sheet,
            record_key: request.record_key,
            actual_type: request.actual_type,
            fields: request.fields,
            field_columns: &layout.field_columns,
            id_column: layout.id_column,
        })
        .map_err(table_write_diagnostics_to_api)?;
        let TableWritePlan::AppendRow(row) = plan else {
            return Err(DiagnosticSet::one(diag(
                "LARK-WRITE",
                "internal error: lark insert did not produce an append-row plan",
            )));
        };
        let width = row
            .values
            .iter()
            .map(|(column, _)| *column)
            .max()
            .unwrap_or(layout.id_column);
        let mut values = vec![String::new(); width];
        for (column, value) in row.values {
            if column == 0 {
                return Err(DiagnosticSet::one(diag(
                    "LARK-WRITE",
                    "lark column index must be at least 1",
                )));
            }
            if values.len() < column {
                values.resize(column, String::new());
            }
            values[column - 1] = value;
        }
        self.append_lark_row(&spreadsheet_token, &sheet_id, &values, &auth)?;
        Ok(WriteOutcome {
            touched_record_origins: Vec::new(),
            inserted_record_origin: Some(RecordOrigin::Table {
                document: SourceDocument::Remote(format!("lark:{spreadsheet_token}")),
                sheet,
                row: 0,
                id_column: layout.id_column,
                field_columns: layout.field_columns,
            }),
            deleted_record_origin: None,
            diagnostics: DiagnosticSet::empty(),
        })
    }

    fn delete_record(
        &self,
        _ctx: WriteContext<'_>,
        request: &DeleteRecordRequest<'_>,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        let RecordOrigin::Table {
            document,
            sheet,
            row,
            id_column,
            ..
        } = request.origin
        else {
            return Err(DiagnosticSet::one(diag(
                "LARK-WRITE",
                "lark writer requires a Table origin",
            )));
        };
        let SourceDocument::Remote(doc) = document else {
            return Err(DiagnosticSet::one(diag(
                "LARK-WRITE",
                "lark writer requires a remote table document",
            )));
        };
        let auth = self.lark_write_auth(request.source)?;
        let spreadsheet_token =
            self.lark_spreadsheet_token_from_source(request.source, &auth.token)?;
        let same_source_uri =
            matches!(&request.source.location, SourceLocationSpec::Uri(uri) if uri == doc);
        let same_spreadsheet =
            lark_document_spreadsheet_token(doc).as_deref() == Some(spreadsheet_token.as_str());
        if !same_source_uri && !same_spreadsheet {
            return Err(DiagnosticSet::one(diag(
                "LARK-WRITE",
                "record origin does not belong to the requested lark source",
            )));
        }
        if *id_column == 0 || *row == 0 {
            return Err(DiagnosticSet::one(diag(
                "LARK-WRITE",
                "lark row and id column indexes must be at least 1",
            )));
        }
        let sheet_id = self.cached_sheet_id(&spreadsheet_token, sheet, &auth.token)?;
        let current_key =
            self.read_lark_cell(&spreadsheet_token, &sheet_id, *row, *id_column, &auth)?;
        if current_key.trim() != request.record_key {
            return Err(DiagnosticSet::one(diag(
                "LARK-WRITE",
                format!(
                    "row {row} in lark sheet `{sheet}` expected key `{}` but found `{}`",
                    request.record_key,
                    current_key.trim()
                ),
            )));
        }
        self.delete_lark_row(&spreadsheet_token, &sheet_id, *row, &auth)?;
        Ok(WriteOutcome {
            touched_record_origins: Vec::new(),
            inserted_record_origin: None,
            deleted_record_origin: Some(request.origin.clone()),
            diagnostics: DiagnosticSet::empty(),
        })
    }

    fn rewrite_record_references(
        &self,
        _ctx: WriteContext<'_>,
        _request: &RewriteRecordReferencesRequest<'_>,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        Ok(WriteOutcome::default())
    }
}

impl<C> TableManager for LarkSheetWriter<C>
where
    C: LarkHttpClient + Send + Sync,
{
    fn descriptor(&self) -> &'static TableManagerDescriptor {
        &LARK_SHEET_TABLE_MANAGER_DESCRIPTOR
    }

    fn type_for_sheet(
        &self,
        source: &coflow_api::ResolvedSource,
        sheet: Option<&str>,
    ) -> Result<Option<String>, DiagnosticSet> {
        type_for_sheet_from_options(lark_source_options(source)?, sheet)
    }

    fn sheet_for_type(
        &self,
        source: &coflow_api::ResolvedSource,
        actual_type: &str,
    ) -> Result<Option<String>, DiagnosticSet> {
        sheet_for_type_from_options(lark_source_options(source)?, actual_type)
    }

    fn header_options(
        &self,
        source: &coflow_api::ResolvedSource,
        sheet: &str,
        actual_type: &str,
    ) -> Result<TableHeaderOptions, DiagnosticSet> {
        Ok(table_header_options(sheet_config_from_options(
            lark_source_options(source)?,
            sheet,
            actual_type,
        )?))
    }

    fn create_table(
        &self,
        _ctx: TableContext<'_>,
        request: &CreateTableRequest<'_>,
    ) -> Result<TableOperationResult, DiagnosticSet> {
        let auth = self.lark_write_auth(request.source)?;
        let spreadsheet_token =
            self.lark_spreadsheet_token_from_source(request.source, &auth.token)?;
        if self
            .cached_sheet_id(&spreadsheet_token, request.sheet, &auth.token)
            .is_ok()
        {
            return Err(DiagnosticSet::one(diag(
                "LARK-WRITE",
                format!("sheet `{}` already exists", request.sheet),
            )));
        }
        let sheet_id = self.create_lark_sheet(&spreadsheet_token, request.sheet, &auth)?;
        self.write_lark_header(&spreadsheet_token, &sheet_id, request.headers, &auth)?;
        self.invalidate_caches(None, Some(&spreadsheet_token));
        Ok(TableOperationResult {
            headers: request.headers.to_vec(),
            added: request.headers.to_vec(),
            removed: Vec::new(),
            diagnostics: DiagnosticSet::empty(),
        })
    }

    fn sync_header(
        &self,
        _ctx: TableContext<'_>,
        request: &SyncHeaderRequest<'_>,
    ) -> Result<TableOperationResult, DiagnosticSet> {
        let auth = self.lark_write_auth(request.source)?;
        let spreadsheet_token =
            self.lark_spreadsheet_token_from_source(request.source, &auth.token)?;
        let sheet = request.sheet.unwrap_or(request.actual_type);
        let metadata = self.cached_sheet_metadata(&spreadsheet_token, sheet, &auth.token)?;
        let mut old_header =
            self.read_lark_header(&spreadsheet_token, &metadata.sheet_id, &auth.token)?;
        old_header.resize(metadata.column_count().max(old_header.len()), String::new());
        let plan = HeaderReconciliationPlan::new(&old_header, request.headers);
        let source_rows = self.read_lark_rows(
            &spreadsheet_token,
            &metadata.sheet_id,
            plan.source_width(),
            metadata.row_count(),
            &auth.token,
        )?;
        let mut target_rows = plan.project_rows(&source_rows);
        for row in &mut target_rows {
            row.resize(plan.storage_width(), String::new());
        }
        self.write_lark_rows(
            &spreadsheet_token,
            &metadata.sheet_id,
            &target_rows,
            plan.storage_width(),
            &auth,
        )?;
        Ok(TableOperationResult {
            headers: request.headers.to_vec(),
            added: plan.added().to_vec(),
            removed: plan.removed().to_vec(),
            diagnostics: DiagnosticSet::empty(),
        })
    }
}

fn table_header_options(config: TableSheetConfig) -> TableHeaderOptions {
    let mut out = TableHeaderOptions::new(config.sheet);
    if let Some(type_name) = config.type_name {
        out = out.with_type(type_name);
    }
    if let Some(key) = config.key {
        out = out.with_key(key);
    }
    out.with_columns(config.columns)
}
