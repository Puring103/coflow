use coflow_api::DiagnosticSet;
use serde_json::{json, Value};

use crate::diagnostics::diag;
use crate::dto::{ApiEnvelope, ValuesData};
use crate::http::LarkHttpClient;
use crate::remote::{LarkAuth, LarkHttpMethod, LarkRequest};
use crate::{
    api_error_message, column_name, json_cell_text, url_component, LarkSheetWriter, API_BASE,
};

impl<C> LarkSheetWriter<C>
where
    C: LarkHttpClient + Send + Sync,
{
    pub(crate) fn append_lark_row(
        &self,
        spreadsheet_token: &str,
        sheet_id: &str,
        values: &[String],
        auth: &LarkAuth,
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
        auth: &LarkAuth,
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
        self.invalidate_document(auth, spreadsheet_token);
        self.cached_sheet_id(spreadsheet_token, sheet, auth)
            .map_err(|_| {
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
        auth: &LarkAuth,
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

    pub(crate) fn write_lark_rows(
        &self,
        spreadsheet_token: &str,
        sheet_id: &str,
        rows: &[Vec<String>],
        width: usize,
        auth: &LarkAuth,
    ) -> Result<(), DiagnosticSet> {
        let last_column = column_name(width.max(1));
        let last_row = rows.len().max(1);
        let endpoint = format!(
            "{API_BASE}/sheets/v2/spreadsheets/{}/values",
            url_component(spreadsheet_token)
        );
        let body = json!({
            "valueRange": {
                "range": format!("{sheet_id}!A1:{last_column}{last_row}"),
                "values": rows,
            }
        });
        self.send_lark_write("values", &endpoint, &body, auth, LarkHttpMethod::Put)
    }

    pub(crate) fn delete_lark_row(
        &self,
        spreadsheet_token: &str,
        sheet_id: &str,
        row: usize,
        auth: &LarkAuth,
    ) -> Result<(), DiagnosticSet> {
        if row == 0 {
            return Err(DiagnosticSet::one(diag(
                "LARK-WRITE",
                "lark row index must be at least 1",
            )));
        }
        let endpoint = format!(
            "{API_BASE}/sheets/v2/spreadsheets/{}/dimension_range",
            url_component(spreadsheet_token)
        );
        let body = json!({
            "dimension": {
                "sheetId": sheet_id,
                "majorDimension": "ROWS",
                "startIndex": row,
                "endIndex": row + 1,
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
        auth: &LarkAuth,
    ) -> Result<String, DiagnosticSet> {
        let column_letters = column_name(column);
        let range = format!("{sheet_id}!{column_letters}{row}:{column_letters}{row}");
        let endpoint = format!(
            "{API_BASE}/sheets/v2/spreadsheets/{}/values/{}?valueRenderOption=ToString",
            url_component(spreadsheet_token),
            url_component(&range)
        );
        let response = self.send_remote_request(
            auth,
            &LarkRequest {
                method: LarkHttpMethod::Get,
                code: "LARK-WRITE",
                description: "read id cell before delete",
                endpoint: &endpoint,
                body: None,
            },
        )?;
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
        auth: &LarkAuth,
    ) -> Result<Vec<String>, DiagnosticSet> {
        const HEADER_SCAN_COLUMNS: usize = 256;
        let last_column = column_name(HEADER_SCAN_COLUMNS);
        let range = format!("{sheet_id}!A1:{last_column}1");
        let endpoint = format!(
            "{API_BASE}/sheets/v2/spreadsheets/{}/values/{}?valueRenderOption=ToString",
            url_component(spreadsheet_token),
            url_component(&range)
        );
        let response = self.send_remote_request(
            auth,
            &LarkRequest {
                method: LarkHttpMethod::Get,
                code: "LARK-WRITE",
                description: "read lark header row",
                endpoint: &endpoint,
                body: None,
            },
        )?;
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

    pub(crate) fn read_lark_rows(
        &self,
        spreadsheet_token: &str,
        sheet_id: &str,
        width: usize,
        row_count: usize,
        auth: &LarkAuth,
    ) -> Result<Vec<Vec<String>>, DiagnosticSet> {
        let last_column = column_name(width.max(1));
        let range = format!("{sheet_id}!A1:{last_column}{}", row_count.max(1));
        let endpoint = format!(
            "{API_BASE}/sheets/v2/spreadsheets/{}/values/{}?valueRenderOption=ToString",
            url_component(spreadsheet_token),
            url_component(&range)
        );
        let response = self.send_remote_request(
            auth,
            &LarkRequest {
                method: LarkHttpMethod::Get,
                code: "LARK-WRITE",
                description: "read lark table before header sync",
                endpoint: &endpoint,
                body: None,
            },
        )?;
        let envelope: ApiEnvelope<ValuesData> = serde_json::from_str(&response).map_err(|err| {
            DiagnosticSet::one(diag(
                "LARK-WRITE",
                format!("failed to parse lark table before header sync: {err}"),
            ))
        })?;
        if envelope.code != 0 {
            return Err(DiagnosticSet::one(diag(
                "LARK-WRITE",
                api_error_message(
                    "read lark table before header sync",
                    envelope.code,
                    envelope.msg.as_deref(),
                ),
            )));
        }
        let data = envelope.data.ok_or_else(|| {
            DiagnosticSet::one(diag(
                "LARK-WRITE",
                "read lark table response did not include `data`",
            ))
        })?;
        Ok(data
            .value_range
            .values
            .into_iter()
            .map(|row| row.into_iter().map(json_cell_text).collect())
            .collect())
    }

    pub(crate) fn send_lark_write(
        &self,
        operation: &'static str,
        endpoint: &str,
        body: &Value,
        auth: &LarkAuth,
        method: LarkHttpMethod,
    ) -> Result<(), DiagnosticSet> {
        let response = self.send_remote_request(
            auth,
            &LarkRequest {
                method,
                code: "LARK-WRITE",
                description: operation,
                endpoint,
                body: Some(body),
            },
        )?;
        parse_write_envelope(operation, &response)
    }

    pub(crate) fn send_values_batch_update(
        &self,
        endpoint: &str,
        body: &Value,
        auth: &LarkAuth,
    ) -> Result<(), DiagnosticSet> {
        let response = self.send_remote_request(
            auth,
            &LarkRequest {
                method: LarkHttpMethod::Post,
                code: "LARK-WRITE",
                description: "values_batch_update",
                endpoint,
                body: Some(body),
            },
        )?;
        parse_write_envelope("values_batch_update", &response)
    }
}

fn parse_write_envelope(operation: &'static str, response: &str) -> Result<(), DiagnosticSet> {
    let envelope: ApiEnvelope<Value> = serde_json::from_str(response).map_err(|err| {
        DiagnosticSet::one(diag(
            "LARK-WRITE",
            format!("failed to parse {operation} response: {err}"),
        ))
    })?;
    if envelope.code == 0 {
        return Ok(());
    }
    Err(DiagnosticSet::one(diag(
        "LARK-WRITE",
        api_error_message(operation, envelope.code, envelope.msg.as_deref()),
    )))
}
