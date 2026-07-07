use coflow_api::DiagnosticSet;
use serde_json::{json, Value};

use crate::diagnostics::diag;
use crate::dto::{ApiEnvelope, ValuesData};
use crate::http::LarkHttpClient;
use crate::writer_cache::{fetch_sheet_id_map, LarkWriteAuth};
use crate::{
    api_error_message, column_name, json_cell_text, url_component, LarkSheetWriter, API_BASE,
};

pub(crate) enum LarkWriteFailure {
    TokenExpired(DiagnosticSet),
    Other(DiagnosticSet),
}

impl<C> LarkSheetWriter<C>
where
    C: LarkHttpClient + Send + Sync,
{
    pub(crate) fn append_lark_row(
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

    pub(crate) fn create_lark_sheet(
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

    pub(crate) fn write_lark_header(
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

    pub(crate) fn delete_lark_row(
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

    pub(crate) fn read_lark_cell(
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

    pub(crate) fn read_lark_header(
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

    pub(crate) fn send_values_batch_update(
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
