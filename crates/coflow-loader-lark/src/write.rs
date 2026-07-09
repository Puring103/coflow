use coflow_api::{
    CreateTableRequest, DeleteRecordRequest, DiagnosticSet, InsertRecordRequest, RecordOrigin,
    RenameRecordRequest, RewriteRecordReferencesRequest, SourceDocument, SourceLocationSpec,
    SourceWriter, SyncHeaderRequest, TableAddressing, TableContext, TableManager,
    TableManagerDescriptor, TableOperationResult, WriteCellRequest, WriteContext,
    WriteFieldPathSegment, WriteOutcome, WriterCapabilities, WriterDescriptor,
};
use coflow_loader_table_core::cell_value::render_cell_value;
use coflow_loader_table_core::writer::{plan_insert_record, TableInsertRecord, TableWritePlan};
use serde_json::json;

use crate::diagnostics::{diag, lark_render_error, table_write_diagnostics_to_api};
use crate::http::LarkHttpClient;
use crate::source::{
    lark_document_spreadsheet_token, required_option_string, sheet_for_type_from_options,
};
use crate::write_http::LarkWriteFailure;
use crate::write_layout::{lark_insert_layout, resolve_lark_column, LarkInsertLayoutRequest};
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

        let column = resolve_lark_column(request.field_path, field_columns, *id_column)
            .ok_or_else(|| {
                DiagnosticSet::one(diag(
                    "LARK-WRITE",
                    format!(
                        "field path {:?} does not map to any column in the source row",
                        request.field_path
                    ),
                ))
            })?;
        let cell_value = render_cell_value(request.new_value).map_err(lark_render_error)?;

        let app_id = required_option_string(&request.source.options, "app_id")?;
        let app_secret = required_option_string(&request.source.options, "app_secret")?;
        let token = self.cached_tenant_token(&app_id, &app_secret)?;
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

        let sheet_id = self.cached_sheet_id(&spreadsheet_token, sheet, &token)?;
        let column_letters = column_name(column);
        let range = format!("{sheet_id}!{column_letters}{row}:{column_letters}{row}");
        let endpoint = format!(
            "{API_BASE}/sheets/v2/spreadsheets/{}/values_batch_update",
            url_component(&spreadsheet_token)
        );
        let body = json!({
            "valueRanges": [
                { "range": range, "values": [[cell_value]] }
            ]
        });
        let outcome = match self.send_values_batch_update(&endpoint, &body, &token) {
            Ok(()) => Ok(()),
            Err(LarkWriteFailure::TokenExpired(diag_set)) => {
                self.invalidate_caches(Some(&app_id), None);
                let fresh = self.cached_tenant_token(&app_id, &app_secret)?;
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
        let value = coflow_api::CfdValue::String(request.new_key.to_string());
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
        let sheet = request
            .sheet
            .or_else(|| sheet_for_type_from_options(&request.source.options, request.actual_type))
            .unwrap_or(request.actual_type);
        let sheet_id = self.cached_sheet_id(&spreadsheet_token, sheet, &auth.token)?;
        let layout = lark_insert_layout(&LarkInsertLayoutRequest {
            ctx,
            writer: self,
            source: request.source,
            spreadsheet_token: &spreadsheet_token,
            sheet_id: &sheet_id,
            sheet,
            actual_type: request.actual_type,
            token: &auth.token,
        })?;
        let plan = plan_insert_record(&TableInsertRecord {
            document: SourceDocument::Remote(format!("lark:{spreadsheet_token}")),
            sheet,
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
                sheet: sheet.to_string(),
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
        let sheet_id = self.cached_sheet_id(&spreadsheet_token, sheet, &auth.token)?;
        let old_header = self.read_lark_header(&spreadsheet_token, &sheet_id, &auth.token)?;
        let added = added_columns(request.headers, &old_header);
        let removed = removed_columns(request.headers, &old_header);
        self.write_lark_header(&spreadsheet_token, &sheet_id, request.headers, &auth)?;
        Ok(TableOperationResult {
            headers: request.headers.to_vec(),
            added,
            removed,
            diagnostics: DiagnosticSet::empty(),
        })
    }
}

fn added_columns(new_header: &[String], old_header: &[String]) -> Vec<String> {
    let old = old_header.iter().collect::<std::collections::BTreeSet<_>>();
    new_header
        .iter()
        .filter(|header| !old.contains(header))
        .cloned()
        .collect()
}

fn removed_columns(new_header: &[String], old_header: &[String]) -> Vec<String> {
    let new = new_header.iter().collect::<std::collections::BTreeSet<_>>();
    old_header
        .iter()
        .filter(|header| !new.contains(header))
        .cloned()
        .collect()
}
