//! Round-trip tests for `LarkSheetWriter`: mock the Lark HTTP API with a
//! scripted client, write a cell, assert the writer issued the right
//! sequence of calls (auth → sheets/query → `values_batch_update`), and
//! verify the cache short-circuits the second write.
#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::panic_in_result_fn,
    clippy::unwrap_used
)]

use coflow_api::{
    CfdValue, DeleteRecordRequest, InsertRecordRequest, RecordOrigin, ResolvedSource,
    RewriteRecordReferencesRequest, SourceDocument, SourceLocationSpec, SourceWriter,
    WriteCellRequest, WriteContext, WriteFieldPathSegment,
};
use coflow_api::{CftContainer, ModuleId};
use coflow_loader_lark::{LarkHttpClient, LarkSheetWriter};
use serde_json::Value;
use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
struct ScriptedResponse {
    method: &'static str,
    url_contains: &'static str,
    body: &'static str,
}

impl ScriptedResponse {
    const fn get(url_contains: &'static str, body: &'static str) -> Self {
        Self {
            method: "GET",
            url_contains,
            body,
        }
    }
    const fn post(url_contains: &'static str, body: &'static str) -> Self {
        Self {
            method: "POST",
            url_contains,
            body,
        }
    }
    const fn put(url_contains: &'static str, body: &'static str) -> Self {
        Self {
            method: "PUT",
            url_contains,
            body,
        }
    }
}

#[derive(Debug, Default)]
struct Inner {
    queue: VecDeque<ScriptedResponse>,
    log: Vec<(&'static str, String, Option<Value>)>,
}

/// Test HTTP client. Implements `LarkHttpClient` directly. Cloning the
/// outer `Arc` lets tests both pass it to a writer and inspect call history
/// after writes complete.
#[derive(Debug, Clone)]
struct ScriptedClient(Arc<Mutex<Inner>>);

impl ScriptedClient {
    fn new(responses: impl IntoIterator<Item = ScriptedResponse>) -> Self {
        Self(Arc::new(Mutex::new(Inner {
            queue: responses.into_iter().collect(),
            log: Vec::new(),
        })))
    }
    fn next(&self, method: &'static str, url: &str) -> Result<String, String> {
        let mut inner = self.0.lock().unwrap();
        let response = inner
            .queue
            .pop_front()
            .ok_or_else(|| format!("unexpected {method} {url}"))?;
        if response.method != method || !url.contains(response.url_contains) {
            return Err(format!(
                "expected {} *{}*, got {method} {url}",
                response.method, response.url_contains
            ));
        }
        inner.log.push((method, url.to_string(), None));
        drop(inner);
        Ok(response.body.to_string())
    }
    fn next_json(&self, method: &'static str, url: &str, body: &Value) -> Result<String, String> {
        let mut inner = self.0.lock().unwrap();
        let response = inner
            .queue
            .pop_front()
            .ok_or_else(|| format!("unexpected {method} {url}"))?;
        if response.method != method || !url.contains(response.url_contains) {
            return Err(format!(
                "expected {} *{}*, got {method} {url}",
                response.method, response.url_contains
            ));
        }
        inner
            .log
            .push((method, url.to_string(), Some(body.clone())));
        drop(inner);
        Ok(response.body.to_string())
    }
    fn calls(&self) -> Vec<(&'static str, String, Option<Value>)> {
        self.0.lock().unwrap().log.clone()
    }
    fn remaining(&self) -> usize {
        self.0.lock().unwrap().queue.len()
    }
}

impl LarkHttpClient for ScriptedClient {
    fn get(&self, url: &str, _tenant_access_token: &str) -> Result<String, String> {
        self.next("GET", url)
    }
    fn post_json(
        &self,
        url: &str,
        body: &Value,
        _tenant_access_token: Option<&str>,
    ) -> Result<String, String> {
        self.next_json("POST", url, body)
    }
    fn delete_json(
        &self,
        url: &str,
        body: &Value,
        _tenant_access_token: &str,
    ) -> Result<String, String> {
        self.next_json("DELETE", url, body)
    }
    fn put_json(
        &self,
        url: &str,
        body: &Value,
        _tenant_access_token: &str,
    ) -> Result<String, String> {
        self.next_json("PUT", url, body)
    }
}

fn lark_origin() -> RecordOrigin {
    let mut field_columns = BTreeMap::new();
    field_columns.insert(vec!["name".to_string()], 2);
    RecordOrigin::Table {
        document: SourceDocument::Remote("lark:sht_test".to_string()),
        sheet: "Items".to_string(),
        row: 2,
        id_column: 1,
        field_columns,
    }
}

fn lark_source() -> ResolvedSource {
    ResolvedSource {
        provider_id: "lark-sheet".to_string(),
        location: SourceLocationSpec::Uri("lark:sht_test".to_string()),
        options: serde_json::json!({
            "app_id": "cli_test",
            "app_secret": "secret_test"
        }),
        display_name: "lark:sht_test".to_string(),
    }
}

fn lark_wiki_source() -> ResolvedSource {
    ResolvedSource {
        provider_id: "lark-sheet".to_string(),
        location: SourceLocationSpec::Uri("https://example.feishu.cn/wiki/wiki_token".to_string()),
        options: serde_json::json!({
            "app_id": "cli_test",
            "app_secret": "secret_test"
        }),
        display_name: "https://example.feishu.cn/wiki/wiki_token".to_string(),
    }
}

fn lark_wiki_origin() -> RecordOrigin {
    let mut field_columns = BTreeMap::new();
    field_columns.insert(vec!["name".to_string()], 2);
    RecordOrigin::Table {
        document: SourceDocument::Remote("https://example.feishu.cn/wiki/wiki_token".to_string()),
        sheet: "Items".to_string(),
        row: 2,
        id_column: 1,
        field_columns,
    }
}

#[test]
fn writes_cell_with_full_handshake_then_caches() {
    // First write: 3 round-trips (auth → sheets/query → values_batch_update).
    // Second write: 1 round-trip (just values_batch_update); token + sheet
    // metadata are cached.
    let client = ScriptedClient::new([
        ScriptedResponse::post(
            "auth/v3/tenant_access_token/internal",
            r#"{"code":0,"tenant_access_token":"tk_first","expire":7200}"#,
        ),
        ScriptedResponse::get(
            "/sheets/v3/spreadsheets/sht_test/sheets/query",
            r#"{"code":0,"data":{"sheets":[{"sheet_id":"shtid_items","title":"Items","grid_properties":{"row_count":2,"column_count":3}}]}}"#,
        ),
        ScriptedResponse::post(
            "/sheets/v2/spreadsheets/sht_test/values_batch_update",
            r#"{"code":0,"data":{}}"#,
        ),
        ScriptedResponse::post(
            "/sheets/v2/spreadsheets/sht_test/values_batch_update",
            r#"{"code":0,"data":{}}"#,
        ),
    ]);
    let writer = LarkSheetWriter::new(client.clone());
    let schema = CftContainer::new();
    let source = lark_source();
    let origin = lark_origin();
    let new_value = CfdValue::String("New".to_string());
    let segments = vec![WriteFieldPathSegment::Field("name".to_string())];
    let request = WriteCellRequest {
        origin: &origin,
        record_key: "sword",
        actual_type: "Item",
        field_path: &segments,
        new_value: &new_value,
        schema: &schema,
        source: &source,
    };
    let ctx = WriteContext {
        project_root: std::path::Path::new("."),
        schema: &schema,
        model: None,
    };
    writer.write_field(ctx, &request).expect("first write");
    writer
        .write_field(ctx, &request)
        .expect("second write (cached)");

    assert_eq!(client.remaining(), 0, "all responses consumed");
    let calls = client.calls();
    assert_eq!(calls.len(), 4, "first write 3 RTTs + second 1 RTT");
    assert!(calls[0].1.contains("tenant_access_token"));
    assert!(calls[1].1.contains("/sheets/query"));
    assert!(calls[2].1.contains("values_batch_update"));
    assert!(calls[3].1.contains("values_batch_update"));
}

#[test]
fn writes_cell_from_wiki_url_origin() {
    let client = ScriptedClient::new([
        ScriptedResponse::post(
            "auth/v3/tenant_access_token/internal",
            r#"{"code":0,"tenant_access_token":"tk","expire":7200}"#,
        ),
        ScriptedResponse::get(
            "/wiki/v2/spaces/get_node?token=wiki_token",
            r#"{"code":0,"data":{"node":{"obj_type":"sheet","obj_token":"sht_test"}}}"#,
        ),
        ScriptedResponse::get(
            "/sheets/v3/spreadsheets/sht_test/sheets/query",
            r#"{"code":0,"data":{"sheets":[{"sheet_id":"shtid_items","title":"Items","grid_properties":{"row_count":2,"column_count":3}}]}}"#,
        ),
        ScriptedResponse::post(
            "/sheets/v2/spreadsheets/sht_test/values_batch_update",
            r#"{"code":0,"data":{}}"#,
        ),
    ]);
    let writer = LarkSheetWriter::new(client.clone());
    let schema = CftContainer::new();
    let source = lark_wiki_source();
    let origin = lark_wiki_origin();
    let new_value = CfdValue::String("New".to_string());
    let segments = vec![WriteFieldPathSegment::Field("name".to_string())];
    let request = WriteCellRequest {
        origin: &origin,
        record_key: "sword",
        actual_type: "Item",
        field_path: &segments,
        new_value: &new_value,
        schema: &schema,
        source: &source,
    };
    let ctx = WriteContext {
        project_root: std::path::Path::new("."),
        schema: &schema,
        model: None,
    };

    writer.write_field(ctx, &request).expect("write wiki row");

    assert_eq!(client.remaining(), 0);
    let calls = client.calls();
    assert_eq!(calls.len(), 4);
    assert!(calls[1].1.contains("/wiki/v2/spaces/get_node"));
    assert!(calls[3].1.contains("values_batch_update"));
}

#[test]
fn surfaces_business_error_on_failure() {
    let client = ScriptedClient::new([
        ScriptedResponse::post(
            "auth/v3/tenant_access_token/internal",
            r#"{"code":0,"tenant_access_token":"tk","expire":7200}"#,
        ),
        ScriptedResponse::get(
            "/sheets/v3/spreadsheets/sht_test/sheets/query",
            r#"{"code":0,"data":{"sheets":[{"sheet_id":"shtid_items","title":"Items","grid_properties":{"row_count":2,"column_count":3}}]}}"#,
        ),
        ScriptedResponse::post(
            "/sheets/v2/spreadsheets/sht_test/values_batch_update",
            r#"{"code":91402,"msg":"sheet not found"}"#,
        ),
    ]);
    let writer = LarkSheetWriter::new(client);
    let schema = CftContainer::new();
    let source = lark_source();
    let origin = lark_origin();
    let new_value = CfdValue::String("X".to_string());
    let segments = vec![WriteFieldPathSegment::Field("name".to_string())];
    let request = WriteCellRequest {
        origin: &origin,
        record_key: "sword",
        actual_type: "Item",
        field_path: &segments,
        new_value: &new_value,
        schema: &schema,
        source: &source,
    };
    let ctx = WriteContext {
        project_root: std::path::Path::new("."),
        schema: &schema,
        model: None,
    };
    let Err(diag) = writer.write_field(ctx, &request) else {
        panic!("error envelope must surface as diagnostic");
    };
    assert!(diag.iter().any(|d| d.message.contains("sheet not found")));
}

#[test]
fn retries_once_after_token_expired() {
    // First values_batch_update returns a stale-token error; the writer
    // must invalidate the cached token, re-auth, and retry.
    let client = ScriptedClient::new([
        ScriptedResponse::post(
            "auth/v3/tenant_access_token/internal",
            r#"{"code":0,"tenant_access_token":"tk_old","expire":7200}"#,
        ),
        ScriptedResponse::get(
            "/sheets/v3/spreadsheets/sht_test/sheets/query",
            r#"{"code":0,"data":{"sheets":[{"sheet_id":"shtid_items","title":"Items","grid_properties":{"row_count":2,"column_count":3}}]}}"#,
        ),
        ScriptedResponse::post(
            "/sheets/v2/spreadsheets/sht_test/values_batch_update",
            r#"{"code":99991663,"msg":"access token expired"}"#,
        ),
        ScriptedResponse::post(
            "auth/v3/tenant_access_token/internal",
            r#"{"code":0,"tenant_access_token":"tk_new","expire":7200}"#,
        ),
        ScriptedResponse::post(
            "/sheets/v2/spreadsheets/sht_test/values_batch_update",
            r#"{"code":0,"data":{}}"#,
        ),
    ]);
    let writer = LarkSheetWriter::new(client.clone());
    let schema = CftContainer::new();
    let source = lark_source();
    let origin = lark_origin();
    let new_value = CfdValue::String("Retry".to_string());
    let segments = vec![WriteFieldPathSegment::Field("name".to_string())];
    let request = WriteCellRequest {
        origin: &origin,
        record_key: "sword",
        actual_type: "Item",
        field_path: &segments,
        new_value: &new_value,
        schema: &schema,
        source: &source,
    };
    let ctx = WriteContext {
        project_root: std::path::Path::new("."),
        schema: &schema,
        model: None,
    };
    writer.write_field(ctx, &request).expect("retry succeeds");
    assert_eq!(client.remaining(), 0, "retry must consume all responses");
}

fn item_schema() -> CftContainer {
    let mut schema = CftContainer::new();
    schema
        .add_module(
            ModuleId::from("main"),
            "type Item { name: string; power: int; } type Holder { item: &Item; }",
        )
        .expect("schema parse");
    schema.compile().expect("schema compile");
    schema
}

#[test]
fn inserts_record_by_appending_lark_row() {
    let client = ScriptedClient::new([
        ScriptedResponse::post(
            "auth/v3/tenant_access_token/internal",
            r#"{"code":0,"tenant_access_token":"tk","expire":7200}"#,
        ),
        ScriptedResponse::get(
            "/sheets/v3/spreadsheets/sht_test/sheets/query",
            r#"{"code":0,"data":{"sheets":[{"sheet_id":"shtid_items","title":"Items","grid_properties":{"row_count":2,"column_count":3}}]}}"#,
        ),
        ScriptedResponse::get(
            "/sheets/v2/spreadsheets/sht_test/values/shtid_items%21A1%3AIV1?valueRenderOption=ToString",
            r#"{"code":0,"data":{"valueRange":{"values":[["id","name","power"]]}}}"#,
        ),
        ScriptedResponse::post(
            "/sheets/v2/spreadsheets/sht_test/values_append",
            r#"{"code":0,"data":{}}"#,
        ),
    ]);
    let writer = LarkSheetWriter::new(client.clone());
    let schema = item_schema();
    let source = lark_source();
    let fields = BTreeMap::from([
        ("name".to_string(), CfdValue::String("Blade".to_string())),
        ("power".to_string(), CfdValue::Int(7)),
    ]);
    let request = InsertRecordRequest {
        source: &source,
        sheet: Some("Items"),
        record_key: "blade",
        actual_type: "Item",
        fields: &fields,
        schema: &schema,
    };
    let ctx = WriteContext {
        project_root: std::path::Path::new("."),
        schema: &schema,
        model: None,
    };

    writer.insert_record(ctx, &request).expect("insert row");

    assert_eq!(client.remaining(), 0);
    let calls = client.calls();
    let Some(body) = calls
        .iter()
        .find(|(_, url, _)| url.contains("values_append"))
        .and_then(|(_, _, body)| body.as_ref())
    else {
        panic!("values_append body should be recorded");
    };
    assert_eq!(
        body,
        &serde_json::json!({
            "valueRange": {
                "range": "shtid_items!A:C",
                "values": [["blade", "Blade", "7"]],
            }
        })
    );
}

#[test]
fn inserts_record_from_wiki_url_source() {
    let client = ScriptedClient::new([
        ScriptedResponse::post(
            "auth/v3/tenant_access_token/internal",
            r#"{"code":0,"tenant_access_token":"tk","expire":7200}"#,
        ),
        ScriptedResponse::get(
            "/wiki/v2/spaces/get_node?token=wiki_token",
            r#"{"code":0,"data":{"node":{"obj_type":"sheet","obj_token":"sht_test"}}}"#,
        ),
        ScriptedResponse::get(
            "/sheets/v3/spreadsheets/sht_test/sheets/query",
            r#"{"code":0,"data":{"sheets":[{"sheet_id":"shtid_items","title":"Items","grid_properties":{"row_count":2,"column_count":3}}]}}"#,
        ),
        ScriptedResponse::get(
            "/sheets/v2/spreadsheets/sht_test/values/shtid_items%21A1%3AIV1?valueRenderOption=ToString",
            r#"{"code":0,"data":{"valueRange":{"values":[["id","name","power"]]}}}"#,
        ),
        ScriptedResponse::post(
            "/sheets/v2/spreadsheets/sht_test/values_append",
            r#"{"code":0,"data":{}}"#,
        ),
    ]);
    let writer = LarkSheetWriter::new(client.clone());
    let schema = item_schema();
    let source = lark_wiki_source();
    let fields = BTreeMap::from([
        ("name".to_string(), CfdValue::String("Blade".to_string())),
        ("power".to_string(), CfdValue::Int(7)),
    ]);
    let request = InsertRecordRequest {
        source: &source,
        sheet: Some("Items"),
        record_key: "blade",
        actual_type: "Item",
        fields: &fields,
        schema: &schema,
    };
    let ctx = WriteContext {
        project_root: std::path::Path::new("."),
        schema: &schema,
        model: None,
    };

    writer.insert_record(ctx, &request).expect("insert row");

    assert_eq!(client.remaining(), 0);
}

#[test]
fn creates_lark_sheet_and_writes_header() {
    let client = ScriptedClient::new([
        ScriptedResponse::post(
            "auth/v3/tenant_access_token/internal",
            r#"{"code":0,"tenant_access_token":"tk","expire":7200}"#,
        ),
        ScriptedResponse::get(
            "/sheets/v3/spreadsheets/sht_test/sheets/query",
            r#"{"code":0,"data":{"sheets":[{"sheet_id":"shtid_other","title":"Other","grid_properties":{"row_count":1,"column_count":1}}]}}"#,
        ),
        ScriptedResponse::post(
            "/sheets/v2/spreadsheets/sht_test/sheets_batch_update",
            r#"{"code":0,"data":{}}"#,
        ),
        ScriptedResponse::get(
            "/sheets/v3/spreadsheets/sht_test/sheets/query",
            r#"{"code":0,"data":{"sheets":[{"sheet_id":"shtid_items","title":"Items","grid_properties":{"row_count":1,"column_count":3}}]}}"#,
        ),
        ScriptedResponse::put(
            "/sheets/v2/spreadsheets/sht_test/values",
            r#"{"code":0,"data":{}}"#,
        ),
    ]);
    let writer = LarkSheetWriter::new(client.clone());
    let schema = item_schema();
    let source = lark_source();
    let headers = vec!["id".to_string(), "name".to_string(), "power".to_string()];
    let request = coflow_api::CreateTableRequest {
        source: &source,
        sheet: "Items",
        actual_type: "Item",
        headers: &headers,
        schema: &schema,
    };
    let ctx = WriteContext {
        project_root: std::path::Path::new("."),
        schema: &schema,
        model: None,
    };

    writer.create_table(ctx, &request).expect("create table");

    let calls = client.calls();
    let Some((_, _, Some(body))) = calls.iter().find(|(_, url, _)| url.contains("/values")) else {
        panic!("values body should be recorded");
    };
    assert_eq!(
        body,
        &serde_json::json!({
            "valueRange": {
                "range": "shtid_items!A1:C1",
                "values": [["id", "name", "power"]],
            }
        })
    );
    assert_eq!(client.remaining(), 0);
}

#[test]
fn deletes_record_after_remote_key_guard() {
    let client = ScriptedClient::new([
        ScriptedResponse::post(
            "auth/v3/tenant_access_token/internal",
            r#"{"code":0,"tenant_access_token":"tk","expire":7200}"#,
        ),
        ScriptedResponse::get(
            "/sheets/v3/spreadsheets/sht_test/sheets/query",
            r#"{"code":0,"data":{"sheets":[{"sheet_id":"shtid_items","title":"Items","grid_properties":{"row_count":3,"column_count":3}}]}}"#,
        ),
        ScriptedResponse::get(
            "/sheets/v2/spreadsheets/sht_test/values/shtid_items%21A2%3AA2?valueRenderOption=ToString",
            r#"{"code":0,"data":{"valueRange":{"values":[["sword"]]}}}"#,
        ),
        ScriptedResponse {
            method: "DELETE",
            url_contains: "/sheets/v2/spreadsheets/sht_test/dimension_range",
            body: r#"{"code":0,"data":{}}"#,
        },
    ]);
    let writer = LarkSheetWriter::new(client.clone());
    let schema = CftContainer::new();
    let source = lark_source();
    let origin = lark_origin();
    let request = DeleteRecordRequest {
        origin: &origin,
        record_key: "sword",
        actual_type: "Item",
        source: &source,
    };
    let ctx = WriteContext {
        project_root: std::path::Path::new("."),
        schema: &schema,
        model: None,
    };

    writer.delete_record(ctx, &request).expect("delete row");

    assert_eq!(client.remaining(), 0);
    let calls = client.calls();
    let Some(body) = calls
        .iter()
        .find(|(method, url, _)| *method == "DELETE" && url.contains("dimension_range"))
        .and_then(|(_, _, body)| body.as_ref())
    else {
        panic!("dimension_range delete body should be recorded");
    };
    assert_eq!(
        body,
        &serde_json::json!({
            "dimension": {
                "sheetId": "shtid_items",
                "majorDimension": "ROWS",
                "startIndex": 1,
                "endIndex": 2,
            }
        })
    );
}

#[test]
fn rewrite_record_references_does_not_scan_lark_cells() {
    let client = ScriptedClient::new([]);
    let writer = LarkSheetWriter::new(client.clone());
    let schema = item_schema();
    let source = lark_source();
    let targets = [];
    let request = RewriteRecordReferencesRequest {
        source: &source,
        old_key: "sword",
        new_key: "blade",
        targets: &targets,
        schema: &schema,
    };
    let ctx = WriteContext {
        project_root: std::path::Path::new("."),
        schema: &schema,
        model: None,
    };

    writer
        .rewrite_record_references(ctx, &request)
        .expect("rewrite lark source");

    assert_eq!(client.remaining(), 0);
    assert!(client.calls().is_empty());
}
