#![allow(clippy::expect_used, clippy::panic)]

#[path = "../../../tests/support/table_conformance.rs"]
mod table_conformance;

use coflow_api::{
    ResolvedSource, SourceLocationSpec, SourceProvider, SyncHeaderRequest, TableContext,
    TableManager,
};
use coflow_loader_lark::{LarkHttpClient, LarkSheetLoader, LarkSheetWriter};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use table_conformance::{table_conformance_cases, TableConformanceCase};

#[derive(Debug)]
struct ClientState {
    source_rows: Vec<Vec<String>>,
    update: Option<Value>,
}

#[derive(Clone, Debug)]
struct ConformanceClient(Arc<Mutex<ClientState>>);

impl ConformanceClient {
    fn new(case: &TableConformanceCase) -> Self {
        Self(Arc::new(Mutex::new(ClientState {
            source_rows: case.source_rows.clone(),
            update: None,
        })))
    }

    fn update(&self) -> Value {
        self.0
            .lock()
            .expect("lock conformance client")
            .update
            .clone()
            .expect("lark update body")
    }
}

impl LarkHttpClient for ConformanceClient {
    fn get(&self, url: &str, _tenant_access_token: &str) -> Result<String, String> {
        let state = self
            .0
            .lock()
            .map_err(|_| "conformance client poisoned".to_string())?;
        if url.contains("/sheets/query") {
            return Ok(json!({
                "code": 0,
                "data": {
                    "sheets": [{
                        "sheet_id": "shtid_items",
                        "title": "Items",
                        "grid_properties": {
                            "row_count": state.source_rows.len(),
                            "column_count": state.source_rows.first().map_or(0, Vec::len)
                        }
                    }]
                }
            })
            .to_string());
        }
        let values = if url.contains("IV1") {
            state
                .source_rows
                .first()
                .map_or_else(Vec::new, |header| vec![header.clone()])
        } else {
            state.source_rows.clone()
        };
        Ok(json!({"code": 0, "data": {"valueRange": {"values": values}}}).to_string())
    }

    fn post_json(
        &self,
        _url: &str,
        _body: &Value,
        _tenant_access_token: Option<&str>,
    ) -> Result<String, String> {
        Ok(json!({
            "code": 0,
            "tenant_access_token": "token",
            "expire": 7200
        })
        .to_string())
    }

    fn put_json(
        &self,
        _url: &str,
        body: &Value,
        _tenant_access_token: &str,
    ) -> Result<String, String> {
        self.0
            .lock()
            .map_err(|_| "conformance client poisoned".to_string())?
            .update = Some(body.clone());
        Ok(json!({"code": 0, "data": {}}).to_string())
    }
}

fn lark_source() -> ResolvedSource {
    ResolvedSource {
        provider_id: "lark-sheet".to_string(),
        location: SourceLocationSpec::Uri("lark:sht_test".to_string()),
        options: LarkSheetLoader::default()
            .decode_options(&json!({
                "app_id": "conformance",
                "app_secret": "secret"
            }))
            .expect("decode lark options"),
        display_name: "lark:sht_test".to_string(),
    }
}

#[test]
fn lark_table_manager_passes_shared_header_conformance() {
    for case in table_conformance_cases() {
        let client = ConformanceClient::new(&case);
        let writer = LarkSheetWriter::new(client.clone());
        let source = lark_source();
        let result = writer
            .sync_header(
                TableContext {
                    project_root: std::env::temp_dir().as_path(),
                },
                &SyncHeaderRequest {
                    source: &source,
                    sheet: Some("Items"),
                    actual_type: "Item",
                    headers: &case.target_header,
                    schema: None,
                },
            )
            .expect("sync lark header");

        let update = client.update();
        assert_eq!(
            update["valueRange"]["values"],
            json!(case.expected_storage_rows()),
            "case {}",
            case.name
        );
        assert_eq!(result.added, case.added, "case {}", case.name);
        assert_eq!(result.removed, case.removed, "case {}", case.name);
    }
}
