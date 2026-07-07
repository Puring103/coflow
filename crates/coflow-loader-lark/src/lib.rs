//! Feishu/Lark Sheets loader for Coflow table sources.

#![cfg_attr(
    not(test),
    deny(
        clippy::dbg_macro,
        clippy::expect_used,
        clippy::panic,
        clippy::panic_in_result_fn,
        clippy::todo,
        clippy::unimplemented,
        clippy::unreachable,
        clippy::unwrap_used
    )
)]
#![allow(
    clippy::missing_const_for_fn,
    clippy::module_name_repetitions,
    clippy::multiple_crate_versions,
    clippy::struct_field_names
)]

mod diagnostics;
mod dto;
mod http;
mod load;
mod source;

use coflow_api::{
    CreateTableRequest, DataWriter, DeleteRecordRequest, DiagnosticSet, InsertRecordRequest,
    RecordOrigin, RenameRecordRequest, ResolvedSource, RewriteRecordReferencesRequest,
    SourceDocument, SourceLocationSpec, WriteCellRequest, WriteContext, WriteFieldPathSegment,
    WriteOutcome, WriterCapabilities, WriterDescriptor,
};
use coflow_loader_table_core::cell_value::render_cell_value;
use coflow_loader_table_core::writer::{plan_insert_record, TableInsertRecord, TableWritePlan};
use coflow_loader_table_core::{resolve_table_write_layout, TableWriteLayout};
use percent_encoding::{utf8_percent_encode, AsciiSet, CONTROLS};
use serde_json::{json, Value};

use dto::{ApiEnvelope, AuthResponse, SheetsQueryData, ValuesData};
use diagnostics::{
    diag, lark_diagnostics_to_api, lark_render_error, table_diagnostics_to_api,
    table_write_diagnostics_to_api,
};
use source::{
    lark_document_spreadsheet_token, lark_source_from_spec, required_option_string,
    sheet_config_from_options, sheet_for_type_from_options,
};

pub use diagnostics::{LarkDiagnostic, LarkDiagnostics};
pub use http::{LarkHttpClient, UreqLarkHttpClient};
pub use load::{
    load_lark_table_source, load_lark_table_source_with_client, LarkSheetLoader,
    LARK_SHEET_LOADER_DESCRIPTOR,
};
use load::spreadsheet_token;
pub use source::{LarkSheetLocator, LarkSheetSource};

pub(crate) const AUTH_URL: &str =
    "https://open.feishu.cn/open-apis/auth/v3/tenant_access_token/internal";
pub(crate) const API_BASE: &str = "https://open.feishu.cn/open-apis";
const URL_COMPONENT_ENCODE_SET: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'&')
    .add(b'+')
    .add(b'/')
    .add(b':')
    .add(b'<')
    .add(b'>')
    .add(b'?')
    .add(b'@')
    .add(b'[')
    .add(b'\\')
    .add(b']')
    .add(b'^')
    .add(b'`')
    .add(b'{')
    .add(b'|')
    .add(b'}')
    .add(b'!');

pub(crate) fn json_cell_text(value: Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(text) => text,
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::Object(mut object) => object
            .remove("text")
            .and_then(|value| value.as_str().map(str::to_string))
            .unwrap_or_else(|| Value::Object(object).to_string()),
        Value::Array(values) => Value::Array(values).to_string(),
    }
}

pub(crate) fn column_name(column: usize) -> String {
    let mut value = column;
    let mut name = Vec::new();
    while value > 0 {
        value -= 1;
        #[allow(clippy::cast_possible_truncation)]
        let offset = (value % 26) as u8;
        name.push((b'A' + offset) as char);
        value /= 26;
    }
    name.iter().rev().collect()
}

pub(crate) fn url_component(value: &str) -> String {
    utf8_percent_encode(value, URL_COMPONENT_ENCODE_SET).to_string()
}

pub(crate) fn api_error_message(description: &str, code: i64, msg: Option<&str>) -> String {
    msg.map_or_else(
        || format!("{description} API returned code {code}"),
        |message| format!("{description} API returned code {code}: {message}"),
    )
}

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

/// `DataWriter` for [`RecordOrigin::Table`] origins whose document is a
/// `Remote("lark:<spreadsheet_token>")`. Routes the edit through Lark's
/// `values_batch_update` endpoint.
///
/// Holds an in-memory cache of (a) per-app `tenant_access_token`s with their
/// expiry timestamp and (b) per-spreadsheet sheet-title → sheet-id maps so a
/// hot-path write reuses both and only spends one round-trip on the
/// `values_batch_update` itself. Cached tokens are refreshed eagerly with a
/// 60-second safety margin before their declared `expire` time.
#[derive(Debug)]
pub struct LarkSheetWriter<C = UreqLarkHttpClient> {
    client: C,
    cache: std::sync::Mutex<LarkWriterCache>,
}

#[derive(Debug, Default)]
struct LarkWriterCache {
    /// Keyed by `app_id` — values represent a tenant access token + the
    /// instant after which it is considered stale.
    tokens: std::collections::HashMap<String, CachedToken>,
    /// Keyed by `spreadsheet_token` — values are the sheet-title → sheet-id
    /// map captured the first time we hit the spreadsheet.
    sheet_ids: std::collections::HashMap<String, std::collections::HashMap<String, String>>,
}

#[derive(Debug, Clone)]
struct CachedToken {
    token: String,
    /// `Instant` after which the cached token must be refreshed.
    expires_at: std::time::Instant,
}

impl Default for LarkSheetWriter<UreqLarkHttpClient> {
    fn default() -> Self {
        Self {
            client: UreqLarkHttpClient,
            cache: std::sync::Mutex::new(LarkWriterCache::default()),
        }
    }
}

impl<C> LarkSheetWriter<C> {
    #[must_use]
    pub fn new(client: C) -> Self {
        Self {
            client,
            cache: std::sync::Mutex::new(LarkWriterCache::default()),
        }
    }
}

impl<C> LarkSheetWriter<C>
where
    C: LarkHttpClient + Send + Sync,
{
    /// Get a cached tenant access token, refreshing it via the auth endpoint
    /// when the cache misses or the cached value is within 60s of expiry.
    fn cached_tenant_token(&self, app_id: &str, app_secret: &str) -> Result<String, DiagnosticSet> {
        let now = std::time::Instant::now();
        if let Ok(cache) = self.cache.lock() {
            if let Some(entry) = cache.tokens.get(app_id) {
                if entry.expires_at > now {
                    return Ok(entry.token.clone());
                }
            }
        }
        let (token, ttl_secs) = lark_tenant_token_with_ttl(&self.client, app_id, app_secret)?;
        // Refresh 60 s before declared expiry so a token doesn't expire
        // mid-call. Default to a 30-minute TTL when the response omits one.
        let safety_margin = std::time::Duration::from_mins(1);
        let lifetime = ttl_secs.map_or_else(
            || std::time::Duration::from_mins(30),
            std::time::Duration::from_secs,
        );
        let expires_at = now + lifetime.saturating_sub(safety_margin);
        if let Ok(mut cache) = self.cache.lock() {
            cache.tokens.insert(
                app_id.to_string(),
                CachedToken {
                    token: token.clone(),
                    expires_at,
                },
            );
        }
        Ok(token)
    }

    /// Look up the sheet id for a sheet title in a given spreadsheet,
    /// fetching the spreadsheet's metadata once and caching the full
    /// title→id map for subsequent lookups.
    fn cached_sheet_id(
        &self,
        spreadsheet_token: &str,
        sheet_title: &str,
        tenant_token: &str,
    ) -> Result<String, DiagnosticSet> {
        if let Ok(cache) = self.cache.lock() {
            if let Some(map) = cache.sheet_ids.get(spreadsheet_token) {
                if let Some(id) = map.get(sheet_title) {
                    return Ok(id.clone());
                }
                // The same spreadsheet might already be cached, but the
                // particular title has not been resolved yet. Fall through
                // to fetch + insert without invalidating siblings.
            }
        }
        let map = fetch_sheet_id_map(&self.client, spreadsheet_token, tenant_token)?;
        let resolved = map.get(sheet_title).cloned().ok_or_else(|| {
            DiagnosticSet::one(diag(
                "LARK-WRITE",
                format!("sheet `{sheet_title}` not found in spreadsheet"),
            ))
        })?;
        if let Ok(mut cache) = self.cache.lock() {
            cache.sheet_ids.insert(spreadsheet_token.to_string(), map);
        }
        Ok(resolved)
    }

    /// Drop cached entries for an `app_id` / spreadsheet pair after a write
    /// fails with auth or sheet-not-found errors. Called by the writer's
    /// retry path.
    fn invalidate_caches(&self, app_id: Option<&str>, spreadsheet_token: Option<&str>) {
        if let Ok(mut cache) = self.cache.lock() {
            if let Some(app) = app_id {
                cache.tokens.remove(app);
            }
            if let Some(token) = spreadsheet_token {
                cache.sheet_ids.remove(token);
            }
        }
    }

    fn lark_spreadsheet_token_from_source(
        &self,
        source: &ResolvedSource,
        tenant_access_token: &str,
    ) -> Result<String, DiagnosticSet> {
        let lark_source = lark_source_from_spec(source)?;
        match spreadsheet_token(&self.client, &lark_source, tenant_access_token) {
            Ok(token) => Ok(token),
            Err(err) => Err(lark_diagnostics_to_api(err)),
        }
    }
}

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

        // Authenticate using the source's options so writes are scoped to
        // the same tenant as the read path. Tokens are cached per `app_id`
        // and refreshed eagerly before expiry; subsequent writes against
        // the same tenant skip the auth round-trip entirely.
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

        // Resolve sheet title → sheet_id. The map is cached per
        // `spreadsheet_token` so writes after the first only pay for the
        // metadata query once.
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
        // Send the write. If the cached token was rejected as stale,
        // invalidate the cache and retry once with a fresh token.
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

/// Outcome of a single `values_batch_update` HTTP call. Distinguishing
/// "token-expired" from generic failures lets the writer retry exactly once
/// with a fresh token rather than asking the user to retry the whole edit.
enum LarkWriteFailure {
    /// Server reported a stale-credential class error.
    TokenExpired(DiagnosticSet),
    /// Anything else (transport, malformed response, business error).
    Other(DiagnosticSet),
}

impl<C> LarkSheetWriter<C>
where
    C: LarkHttpClient + Send + Sync,
{
    fn lark_write_auth(&self, source: &ResolvedSource) -> Result<LarkWriteAuth, DiagnosticSet> {
        let app_id = required_option_string(&source.options, "app_id")?;
        let app_secret = required_option_string(&source.options, "app_secret")?;
        let token = self.cached_tenant_token(&app_id, &app_secret)?;
        Ok(LarkWriteAuth {
            app_id,
            app_secret,
            token,
        })
    }

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
    // Lark's tenant-token-expired family hovers around 99991663 / 99991668.
    if (99_991_000..100_000_000).contains(&envelope.code) {
        Err(LarkWriteFailure::TokenExpired(diag_set))
    } else {
        Err(LarkWriteFailure::Other(diag_set))
    }
}

#[derive(Debug, Clone)]
struct LarkWriteAuth {
    app_id: String,
    app_secret: String,
    token: String,
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

/// Fetch a tenant access token + the server-declared TTL (in seconds), which
/// the writer cache uses to schedule refreshes.
fn lark_tenant_token_with_ttl(
    client: &impl LarkHttpClient,
    app_id: &str,
    app_secret: &str,
) -> Result<(String, Option<u64>), DiagnosticSet> {
    let body = json!({ "app_id": app_id, "app_secret": app_secret });
    let response = client
        .post_json(AUTH_URL, &body, None)
        .map_err(|message| DiagnosticSet::one(diag("LARK-WRITE", message)))?;
    let envelope: AuthResponse = serde_json::from_str(&response)
        .map_err(|err| DiagnosticSet::one(diag("LARK-WRITE", err.to_string())))?;
    if envelope.code != 0 {
        return Err(DiagnosticSet::one(diag(
            "LARK-WRITE",
            api_error_message(
                "tenant access token",
                envelope.code,
                envelope.msg.as_deref(),
            ),
        )));
    }
    let token = envelope.tenant_access_token.ok_or_else(|| {
        DiagnosticSet::one(diag(
            "LARK-WRITE",
            "tenant access token response did not include `tenant_access_token`",
        ))
    })?;
    Ok((token, envelope.expire))
}

/// Fetch the sheet metadata for a spreadsheet and return a `title → sheet_id`
/// map keyed by sheet title (and also containing `sheet_id → sheet_id`
/// self-entries so callers passing a sheet id directly still get a hit).
fn fetch_sheet_id_map(
    client: &impl LarkHttpClient,
    spreadsheet_token: &str,
    tenant_token: &str,
) -> Result<std::collections::HashMap<String, String>, DiagnosticSet> {
    let endpoint = format!(
        "{API_BASE}/sheets/v3/spreadsheets/{}/sheets/query",
        url_component(spreadsheet_token)
    );
    let response = client
        .get(&endpoint, tenant_token)
        .map_err(|message| DiagnosticSet::one(diag("LARK-WRITE", message)))?;
    let envelope: ApiEnvelope<SheetsQueryData> = serde_json::from_str(&response)
        .map_err(|err| DiagnosticSet::one(diag("LARK-WRITE", err.to_string())))?;
    if envelope.code != 0 {
        return Err(DiagnosticSet::one(diag(
            "LARK-WRITE",
            api_error_message("spreadsheet sheets", envelope.code, envelope.msg.as_deref()),
        )));
    }
    let data = envelope.data.ok_or_else(|| {
        DiagnosticSet::one(diag(
            "LARK-WRITE",
            "spreadsheet sheets response did not include `data`",
        ))
    })?;
    let mut map = std::collections::HashMap::new();
    for sheet in data.sheets {
        map.insert(sheet.title.clone(), sheet.sheet_id.clone());
        map.insert(sheet.sheet_id.clone(), sheet.sheet_id);
    }
    Ok(map)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::panic)]

    use super::*;
    use coflow_api::{
        CftContainer, DataLoader, LoadContext, ModuleId, ProbeResult, ProjectSourceRef,
        SourceResolveContext,
    };
    use std::path::Path;

    #[test]
    fn lark_token_url_source_resolves_to_spreadsheet_token_locator() {
        let source = ResolvedSource {
            provider_id: LARK_SHEET_LOADER_DESCRIPTOR.id.to_string(),
            location: SourceLocationSpec::Uri("lark:sht_direct".to_string()),
            options: json!({
                "app_id": "cli_test",
                "app_secret": "secret_test"
            }),
            display_name: "lark:sht_direct".to_string(),
        };

        let Ok(lark_source) = lark_source_from_spec(&source) else {
            panic!("parse lark source");
        };

        assert_eq!(
            lark_source.locator,
            LarkSheetLocator::SpreadsheetToken("sht_direct".to_string())
        );
    }

    #[test]
    fn explicit_lark_loader_rejects_path_source() {
        let loader = LarkSheetLoader::new(NoopClient);
        let schema = CftContainer::new();
        let source = ResolvedSource {
            provider_id: LARK_SHEET_LOADER_DESCRIPTOR.id.to_string(),
            location: SourceLocationSpec::Path(Path::new("data.xlsx").to_path_buf()),
            options: json!({
                "app_id": "cli_test",
                "app_secret": "secret_test"
            }),
            display_name: "data.xlsx".to_string(),
        };

        let Err(err) = loader.resolve(
            SourceResolveContext {
                project_root: Path::new("."),
                schema: &schema,
            },
            &source,
        ) else {
            panic!("lark path source should fail");
        };

        assert!(err
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("lark source requires `url`")));
    }

    #[test]
    fn lark_probe_ignores_local_path_even_with_lark_options() {
        let loader = LarkSheetLoader::new(NoopClient);
        let option_keys = ["app_id", "app_secret"];
        let location = SourceLocationSpec::Path(Path::new("configs.xlsx").to_path_buf());
        let source = ProjectSourceRef {
            source_type: None,
            location: &location,
            option_keys: &option_keys,
        };

        assert_eq!(loader.probe(&source), ProbeResult::none());
    }

    #[test]
    fn loader_reuses_remote_metadata_cache() -> Result<(), String> {
        let client = SequenceClient::new([
            (
                "POST",
                "auth/v3/tenant_access_token/internal",
                r#"{"code":0,"tenant_access_token":"tk","expire":7200}"#,
            ),
            (
                "GET",
                "/wiki/v2/spaces/get_node?token=wiki_token",
                r#"{"code":0,"data":{"node":{"obj_type":"sheet","obj_token":"sht_test"}}}"#,
            ),
            (
                "GET",
                "/sheets/v3/spreadsheets/sht_test/sheets/query",
                r#"{"code":0,"data":{"sheets":[{"sheet_id":"shtid_items","title":"Items","grid_properties":{"row_count":2,"column_count":2}}]}}"#,
            ),
            (
                "GET",
                "/sheets/v2/spreadsheets/sht_test/values/shtid_items%21A1%3AB2?valueRenderOption=ToString",
                r#"{"code":0,"data":{"valueRange":{"values":[["id","name"],["sword","Sword"]]}}}"#,
            ),
            (
                "GET",
                "/sheets/v2/spreadsheets/sht_test/values/shtid_items%21A1%3AB2?valueRenderOption=ToString",
                r#"{"code":0,"data":{"valueRange":{"values":[["id","name"],["sword","Blade"]]}}}"#,
            ),
        ]);
        let loader = LarkSheetLoader::new(client.clone());
        let schema = item_schema()?;
        let source = ResolvedSource {
            provider_id: LARK_SHEET_LOADER_DESCRIPTOR.id.to_string(),
            location: SourceLocationSpec::Uri(
                "https://example.feishu.cn/wiki/wiki_token".to_string(),
            ),
            options: json!({
                "app_id": "cli_test",
                "app_secret": "secret_test",
                "sheets": [{ "sheet": "Items", "type": "Item" }]
            }),
            display_name: "https://example.feishu.cn/wiki/wiki_token".to_string(),
        };
        let ctx = LoadContext {
            project_root: Path::new("."),
            schema: &schema,
        };

        loader
            .load(ctx, &source)
            .map_err(|err| format!("first load: {err:?}"))?;
        loader
            .load(ctx, &source)
            .map_err(|err| format!("second load: {err:?}"))?;

        let remaining = client.remaining()?;
        if remaining != 0 {
            return Err(format!("expected no remaining responses, got {remaining}"));
        }
        Ok(())
    }

    struct NoopClient;

    impl LarkHttpClient for NoopClient {
        fn post_json(
            &self,
            _url: &str,
            _body: &Value,
            _tenant_access_token: Option<&str>,
        ) -> Result<String, String> {
            Err("unexpected HTTP call".to_string())
        }

        fn get(&self, _url: &str, _tenant_access_token: &str) -> Result<String, String> {
            Err("unexpected HTTP call".to_string())
        }
    }

    #[derive(Debug, Clone)]
    struct SequenceClient(
        std::sync::Arc<std::sync::Mutex<std::collections::VecDeque<SequenceResponse>>>,
    );

    #[derive(Debug, Clone)]
    struct SequenceResponse {
        method: &'static str,
        url_contains: &'static str,
        body: &'static str,
    }

    impl SequenceClient {
        fn new(
            responses: impl IntoIterator<Item = (&'static str, &'static str, &'static str)>,
        ) -> Self {
            Self(std::sync::Arc::new(std::sync::Mutex::new(
                responses
                    .into_iter()
                    .map(|(method, url_contains, body)| SequenceResponse {
                        method,
                        url_contains,
                        body,
                    })
                    .collect(),
            )))
        }

        fn next(&self, method: &'static str, url: &str) -> Result<String, String> {
            let response = {
                let mut queue = self
                    .0
                    .lock()
                    .map_err(|_| "lock sequence client".to_string())?;
                queue
                    .pop_front()
                    .ok_or_else(|| format!("unexpected {method} {url}"))?
            };
            if response.method != method || !url.contains(response.url_contains) {
                return Err(format!(
                    "expected {} *{}*, got {method} {url}",
                    response.method, response.url_contains
                ));
            }
            Ok(response.body.to_string())
        }

        fn remaining(&self) -> Result<usize, String> {
            self.0
                .lock()
                .map(|queue| queue.len())
                .map_err(|_| "lock sequence client".to_string())
        }
    }

    impl LarkHttpClient for SequenceClient {
        fn post_json(
            &self,
            url: &str,
            _body: &Value,
            _tenant_access_token: Option<&str>,
        ) -> Result<String, String> {
            self.next("POST", url)
        }

        fn get(&self, url: &str, _tenant_access_token: &str) -> Result<String, String> {
            self.next("GET", url)
        }
    }

    fn item_schema() -> Result<CftContainer, String> {
        let mut schema = CftContainer::new();
        schema
            .add_module(ModuleId::from("main"), "type Item { name: string; }")
            .map_err(|err| format!("schema parse: {err:?}"))?;
        schema
            .compile()
            .map_err(|err| format!("schema compile: {err:?}"))?;
        Ok(schema)
    }
}
