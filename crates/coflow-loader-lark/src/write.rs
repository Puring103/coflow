use coflow_api::{
    CreateTableRequest, DataWriter, DeleteRecordRequest, DiagnosticSet, InsertRecordRequest,
    RecordOrigin, RenameRecordRequest, ResolvedSource, RewriteRecordReferencesRequest,
    SourceDocument, SourceLocationSpec, WriteCellRequest, WriteContext, WriteFieldPathSegment,
    WriteOutcome, WriterCapabilities, WriterDescriptor,
};
use coflow_loader_table_core::cell_value::render_cell_value;
use coflow_loader_table_core::writer::{plan_insert_record, TableInsertRecord, TableWritePlan};
use coflow_loader_table_core::{resolve_table_write_layout, TableWriteLayout};
use serde_json::{json, Value};

use crate::diagnostics::{
    diag, lark_render_error, table_diagnostics_to_api, table_write_diagnostics_to_api,
};
use crate::dto::{ApiEnvelope, ValuesData};
use crate::http::LarkHttpClient;
use crate::source::{
    lark_document_spreadsheet_token, required_option_string, sheet_config_from_options,
    sheet_for_type_from_options,
};
use crate::writer_cache::{fetch_sheet_id_map, LarkWriteAuth};
use crate::{
    api_error_message, column_name, json_cell_text, url_component, LarkSheetWriter, API_BASE,
};

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
        can_create_table: true,
        requires_full_refresh_after_write: true,
        is_remote: true,
    },
};

impl<C> DataWriter for LarkSheetWriter<C>
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

    fn create_table(
        &self,
        _ctx: WriteContext<'_>,
        request: &CreateTableRequest<'_>,
    ) -> Result<WriteOutcome, DiagnosticSet> {
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
        Ok(WriteOutcome::default())
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

enum LarkWriteFailure {
    TokenExpired(DiagnosticSet),
    Other(DiagnosticSet),
}

impl<C> LarkSheetWriter<C>
where
    C: LarkHttpClient + Send + Sync,
{
    fn append_lark_row(
        &self,
        spreadsheet_token: &str,
        sheet_id: &str,
        values: &[String],
        auth: &LarkWriteAuth,
    ) -> Result<(), DiagnosticSet> {
        let last_column = column_name(values.len().max(1));
        let endpoint = format!(
            "{API_BASE}/sheets/v2/spreadsheets/{}/values_append",
            url_component(spreadsheet_token)
        );
        let body = json!({
            "valueRange": {
                "range": format!("{sheet_id}!A:{last_column}"),
                "values": [values],
            }
        });
        self.send_lark_write(
            "values_append",
            &endpoint,
            &body,
            auth,
            LarkHttpMethod::Post,
        )
    }

    fn create_lark_sheet(
        &self,
        spreadsheet_token: &str,
        sheet: &str,
        auth: &LarkWriteAuth,
    ) -> Result<String, DiagnosticSet> {
        let endpoint = format!(
            "{API_BASE}/sheets/v2/spreadsheets/{}/sheets_batch_update",
            url_component(spreadsheet_token)
        );
        let body = json!({
            "requests": [
                { "addSheet": { "properties": { "title": sheet } } }
            ]
        });
        self.send_lark_write(
            "sheets_batch_update",
            &endpoint,
            &body,
            auth,
            LarkHttpMethod::Post,
        )?;
        let map = fetch_sheet_id_map(&self.client, spreadsheet_token, &auth.token)?;
        map.get(sheet).cloned().ok_or_else(|| {
            DiagnosticSet::one(diag(
                "LARK-WRITE",
                format!("created lark sheet `{sheet}` was not found in metadata"),
            ))
        })
    }

    fn write_lark_header(
        &self,
        spreadsheet_token: &str,
        sheet_id: &str,
        headers: &[String],
        auth: &LarkWriteAuth,
    ) -> Result<(), DiagnosticSet> {
        let last_column = column_name(headers.len().max(1));
        let endpoint = format!(
            "{API_BASE}/sheets/v2/spreadsheets/{}/values",
            url_component(spreadsheet_token)
        );
        let body = json!({
            "valueRange": {
                "range": format!("{sheet_id}!A1:{last_column}1"),
                "values": [headers],
            }
        });
        self.send_lark_write("values", &endpoint, &body, auth, LarkHttpMethod::Put)
    }

    fn delete_lark_row(
        &self,
        spreadsheet_token: &str,
        sheet_id: &str,
        row: usize,
        auth: &LarkWriteAuth,
    ) -> Result<(), DiagnosticSet> {
        let zero_based = row.checked_sub(1).ok_or_else(|| {
            DiagnosticSet::one(diag("LARK-WRITE", "lark row index must be at least 1"))
        })?;
        let endpoint = format!(
            "{API_BASE}/sheets/v2/spreadsheets/{}/dimension_range",
            url_component(spreadsheet_token)
        );
        let body = json!({
            "dimension": {
                "sheetId": sheet_id,
                "majorDimension": "ROWS",
                "startIndex": zero_based,
                "endIndex": zero_based + 1,
            }
        });
        self.send_lark_write(
            "delete dimension_range",
            &endpoint,
            &body,
            auth,
            LarkHttpMethod::Delete,
        )
    }

    fn read_lark_cell(
        &self,
        spreadsheet_token: &str,
        sheet_id: &str,
        row: usize,
        column: usize,
        auth: &LarkWriteAuth,
    ) -> Result<String, DiagnosticSet> {
        let column_letters = column_name(column);
        let range = format!("{sheet_id}!{column_letters}{row}:{column_letters}{row}");
        let endpoint = format!(
            "{API_BASE}/sheets/v2/spreadsheets/{}/values/{}?valueRenderOption=ToString",
            url_component(spreadsheet_token),
            url_component(&range)
        );
        let response = self.client.get(&endpoint, &auth.token).map_err(|message| {
            DiagnosticSet::one(diag(
                "LARK-WRITE",
                format!("read id cell before delete failed: {message}"),
            ))
        })?;
        let envelope: ApiEnvelope<ValuesData> = serde_json::from_str(&response).map_err(|err| {
            DiagnosticSet::one(diag(
                "LARK-WRITE",
                format!("failed to parse id cell response: {err}"),
            ))
        })?;
        if envelope.code != 0 {
            return Err(DiagnosticSet::one(diag(
                "LARK-WRITE",
                api_error_message("read id cell", envelope.code, envelope.msg.as_deref()),
            )));
        }
        let data = envelope.data.ok_or_else(|| {
            DiagnosticSet::one(diag(
                "LARK-WRITE",
                "read id cell response did not include `data`",
            ))
        })?;
        Ok(data
            .value_range
            .values
            .into_iter()
            .next()
            .and_then(|row| row.into_iter().next())
            .map_or_else(String::new, json_cell_text))
    }

    fn read_lark_header(
        &self,
        spreadsheet_token: &str,
        sheet_id: &str,
        token: &str,
    ) -> Result<Vec<String>, DiagnosticSet> {
        const HEADER_SCAN_COLUMNS: usize = 256;
        let last_column = column_name(HEADER_SCAN_COLUMNS);
        let range = format!("{sheet_id}!A1:{last_column}1");
        let endpoint = format!(
            "{API_BASE}/sheets/v2/spreadsheets/{}/values/{}?valueRenderOption=ToString",
            url_component(spreadsheet_token),
            url_component(&range)
        );
        let response = self.client.get(&endpoint, token).map_err(|message| {
            DiagnosticSet::one(diag(
                "LARK-WRITE",
                format!("failed to read lark header row: {message}"),
            ))
        })?;
        let envelope: ApiEnvelope<ValuesData> = serde_json::from_str(&response).map_err(|err| {
            DiagnosticSet::one(diag(
                "LARK-WRITE",
                format!("failed to parse lark header row response: {err}"),
            ))
        })?;
        if envelope.code != 0 {
            return Err(DiagnosticSet::one(diag(
                "LARK-WRITE",
                api_error_message(
                    "read lark header row",
                    envelope.code,
                    envelope.msg.as_deref(),
                ),
            )));
        }
        let data = envelope.data.ok_or_else(|| {
            DiagnosticSet::one(diag(
                "LARK-WRITE",
                "read lark header row response did not include `data`",
            ))
        })?;
        Ok(data
            .value_range
            .values
            .into_iter()
            .next()
            .unwrap_or_default()
            .into_iter()
            .map(json_cell_text)
            .collect())
    }

    fn send_lark_write(
        &self,
        operation: &'static str,
        endpoint: &str,
        body: &Value,
        auth: &LarkWriteAuth,
        method: LarkHttpMethod,
    ) -> Result<(), DiagnosticSet> {
        match self.send_lark_write_once(operation, endpoint, body, &auth.token, method) {
            Ok(()) => Ok(()),
            Err(LarkWriteFailure::TokenExpired(diag_set)) => {
                self.invalidate_caches(Some(&auth.app_id), None);
                let fresh = self.cached_tenant_token(&auth.app_id, &auth.app_secret)?;
                self.send_lark_write_once(operation, endpoint, body, &fresh, method)
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
        }
    }

    fn send_lark_write_once(
        &self,
        operation: &'static str,
        endpoint: &str,
        body: &Value,
        token: &str,
        method: LarkHttpMethod,
    ) -> Result<(), LarkWriteFailure> {
        let response = match method {
            LarkHttpMethod::Post => self.client.post_json(endpoint, body, Some(token)),
            LarkHttpMethod::Put => self.client.put_json(endpoint, body, token),
            LarkHttpMethod::Delete => self.client.delete_json(endpoint, body, token),
        }
        .map_err(|message| {
            LarkWriteFailure::Other(DiagnosticSet::one(diag(
                "LARK-WRITE",
                format!("{operation} failed: {message}"),
            )))
        })?;
        parse_write_envelope(operation, &response)
    }

    fn send_values_batch_update(
        &self,
        endpoint: &str,
        body: &Value,
        token: &str,
    ) -> Result<(), LarkWriteFailure> {
        let response = self
            .client
            .post_json(endpoint, body, Some(token))
            .map_err(|message| {
                LarkWriteFailure::Other(DiagnosticSet::one(diag(
                    "LARK-WRITE",
                    format!("values_batch_update failed: {message}"),
                )))
            })?;
        parse_write_envelope("values_batch_update", &response)
    }
}

fn parse_write_envelope(operation: &'static str, response: &str) -> Result<(), LarkWriteFailure> {
    let envelope: ApiEnvelope<Value> = serde_json::from_str(response).map_err(|err| {
        LarkWriteFailure::Other(DiagnosticSet::one(diag(
            "LARK-WRITE",
            format!("failed to parse {operation} response: {err}"),
        )))
    })?;
    if envelope.code == 0 {
        return Ok(());
    }
    let diag_set = DiagnosticSet::one(diag(
        "LARK-WRITE",
        api_error_message(operation, envelope.code, envelope.msg.as_deref()),
    ));
    if (99_991_000..100_000_000).contains(&envelope.code) {
        Err(LarkWriteFailure::TokenExpired(diag_set))
    } else {
        Err(LarkWriteFailure::Other(diag_set))
    }
}

#[derive(Debug, Clone, Copy)]
enum LarkHttpMethod {
    Post,
    Put,
    Delete,
}

struct LarkInsertLayoutRequest<'a, C> {
    ctx: WriteContext<'a>,
    writer: &'a LarkSheetWriter<C>,
    source: &'a ResolvedSource,
    spreadsheet_token: &'a str,
    sheet_id: &'a str,
    sheet: &'a str,
    actual_type: &'a str,
    token: &'a str,
}

fn lark_insert_layout<C>(
    request: &LarkInsertLayoutRequest<'_, C>,
) -> Result<TableWriteLayout, DiagnosticSet>
where
    C: LarkHttpClient + Send + Sync,
{
    if let Some(model) = request.ctx.model {
        if let Some(layout) = model.records().find_map(|(_, record)| {
            let RecordOrigin::Table {
                document,
                sheet: record_sheet,
                id_column,
                field_columns,
                ..
            } = &record.origin
            else {
                return None;
            };
            let SourceDocument::Remote(doc) = document else {
                return None;
            };
            (lark_document_spreadsheet_token(doc).as_deref() == Some(request.spreadsheet_token)
                && record_sheet == request.sheet
                && record.actual_type() == request.actual_type)
                .then_some(TableWriteLayout {
                    id_column: *id_column,
                    field_columns: field_columns.clone(),
                })
        }) {
            return Ok(layout);
        }
    }
    let header = request.writer.read_lark_header(
        request.spreadsheet_token,
        request.sheet_id,
        request.token,
    )?;
    let config =
        sheet_config_from_options(&request.source.options, request.sheet, request.actual_type)?;
    resolve_table_write_layout(
        request.ctx.schema,
        std::path::Path::new(request.spreadsheet_token),
        &config,
        &header,
    )
    .map_err(table_diagnostics_to_api)
}

fn resolve_lark_column(
    field_path: &[WriteFieldPathSegment],
    field_columns: &std::collections::BTreeMap<Vec<String>, usize>,
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
    if let Some(WriteFieldPathSegment::Field(name)) = field_path.first() {
        if name == "id" {
            return Some(id_column);
        }
    }
    None
}
