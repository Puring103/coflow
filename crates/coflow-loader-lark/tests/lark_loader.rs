#![allow(clippy::missing_const_for_fn, clippy::panic_in_result_fn)]

use coflow_loader_lark::{
    load_lark_table_source_with_client, LarkHttpClient, LarkSheetLocator, LarkSheetSource,
};
use coflow_loader_table_core::TableSheetConfig;
use std::cell::RefCell;
use std::collections::VecDeque;

type TestResult = Result<(), String>;

#[test]
fn loads_configured_wiki_sheet_as_table_source() -> TestResult {
    let client = FakeClient::new([
        Response::post(
            "https://open.feishu.cn/open-apis/auth/v3/tenant_access_token/internal",
            r#"{"code":0,"tenant_access_token":"tenant_token"}"#,
        ),
        Response::get(
            "https://open.feishu.cn/open-apis/wiki/v2/spaces/get_node?token=wiki_token",
            r#"{"code":0,"data":{"node":{"obj_type":"sheet","obj_token":"sht_token","title":"配置"}}}"#,
        ),
        Response::get(
            "https://open.feishu.cn/open-apis/sheets/v3/spreadsheets/sht_token/sheets/query",
            r#"{"code":0,"data":{"sheets":[{"sheet_id":"sheet_a","title":"物品表","grid_properties":{"row_count":2,"column_count":3}}]}}"#,
        ),
        Response::get(
            "https://open.feishu.cn/open-apis/sheets/v2/spreadsheets/sht_token/values/sheet_a%21A1%3AC2?valueRenderOption=ToString",
            r#"{"code":0,"data":{"valueRange":{"values":[["物品ID","名称","稀有度"],["sword_01","铁剑","Rare"]]}}}"#,
        ),
    ]);
    let source = LarkSheetSource::new(
        "cli_test",
        "secret_test",
        LarkSheetLocator::Url("https://fand3tbr90g.feishu.cn/wiki/wiki_token".to_string()),
        vec![TableSheetConfig::new("物品表")
            .with_type("Item")
            .with_key("物品ID")
            .with_columns([("名称", "name"), ("稀有度", "rarity")])],
    );

    let table_source =
        load_lark_table_source_with_client(&source, &client).map_err(|err| format!("{err:?}"))?;

    assert_eq!(table_source.name.to_string_lossy(), "lark:sht_token");
    assert_eq!(table_source.configs.len(), 1);
    assert_eq!(table_source.sheets.len(), 1);
    assert_eq!(table_source.sheets[0].name, "物品表");
    assert_eq!(
        table_source.sheets[0].rows,
        vec![
            vec![
                "物品ID".to_string(),
                "名称".to_string(),
                "稀有度".to_string()
            ],
            vec![
                "sword_01".to_string(),
                "铁剑".to_string(),
                "Rare".to_string()
            ],
        ]
    );
    Ok(())
}

#[test]
fn loads_all_sheets_when_sheet_config_is_omitted() -> TestResult {
    let client = FakeClient::new([
        Response::post(
            "https://open.feishu.cn/open-apis/auth/v3/tenant_access_token/internal",
            r#"{"code":0,"tenant_access_token":"tenant_token"}"#,
        ),
        Response::get(
            "https://open.feishu.cn/open-apis/sheets/v3/spreadsheets/sht_direct/sheets/query",
            r#"{"code":0,"data":{"sheets":[{"sheet_id":"sheet_a","title":"Item","grid_properties":{"row_count":2,"column_count":2}},{"sheet_id":"sheet_b","title":"Quest","grid_properties":{"row_count":1,"column_count":2}}]}}"#,
        ),
        Response::get(
            "https://open.feishu.cn/open-apis/sheets/v2/spreadsheets/sht_direct/values/sheet_a%21A1%3AB2?valueRenderOption=ToString",
            r#"{"code":0,"data":{"valueRange":{"values":[["id","name"],["item_1","Potion"]]}}}"#,
        ),
        Response::get(
            "https://open.feishu.cn/open-apis/sheets/v2/spreadsheets/sht_direct/values/sheet_b%21A1%3AB1?valueRenderOption=ToString",
            r#"{"code":0,"data":{"valueRange":{"values":[["id","name"]]}}}"#,
        ),
    ]);
    let source = LarkSheetSource::new(
        "cli_test",
        "secret_test",
        LarkSheetLocator::SpreadsheetToken("sht_direct".to_string()),
        Vec::new(),
    );

    let table_source =
        load_lark_table_source_with_client(&source, &client).map_err(|err| format!("{err:?}"))?;

    assert_eq!(
        table_source
            .configs
            .iter()
            .map(|sheet| sheet.sheet.as_str())
            .collect::<Vec<_>>(),
        vec!["Item", "Quest"]
    );
    assert_eq!(table_source.sheets.len(), 2);
    Ok(())
}

#[test]
fn rejects_wiki_urls_that_do_not_point_to_a_sheet() -> TestResult {
    let client = FakeClient::new([
        Response::post(
            "https://open.feishu.cn/open-apis/auth/v3/tenant_access_token/internal",
            r#"{"code":0,"tenant_access_token":"tenant_token"}"#,
        ),
        Response::get(
            "https://open.feishu.cn/open-apis/wiki/v2/spaces/get_node?token=wiki_token",
            r#"{"code":0,"data":{"node":{"obj_type":"docx","obj_token":"doc_token","title":"文档"}}}"#,
        ),
    ]);
    let source = LarkSheetSource::new(
        "cli_test",
        "secret_test",
        LarkSheetLocator::Url("https://fand3tbr90g.feishu.cn/wiki/wiki_token".to_string()),
        Vec::new(),
    );

    let err = load_lark_table_source_with_client(&source, &client)
        .err()
        .ok_or_else(|| "non-sheet wiki node should fail".to_string())?;

    assert_eq!(err.diagnostics[0].code, "LARK-WIKI");
    assert!(err.diagnostics[0].message.contains("docx"));
    Ok(())
}

#[derive(Debug, Clone)]
struct Response {
    method: &'static str,
    url: &'static str,
    body: &'static str,
}

impl Response {
    fn get(url: &'static str, body: &'static str) -> Self {
        Self {
            method: "GET",
            url,
            body,
        }
    }

    fn post(url: &'static str, body: &'static str) -> Self {
        Self {
            method: "POST",
            url,
            body,
        }
    }
}

#[derive(Debug)]
struct FakeClient {
    responses: RefCell<VecDeque<Response>>,
}

impl FakeClient {
    fn new(responses: impl IntoIterator<Item = Response>) -> Self {
        Self {
            responses: RefCell::new(responses.into_iter().collect()),
        }
    }

    fn next(&self, method: &str, url: &str) -> Result<String, String> {
        let response = self
            .responses
            .borrow_mut()
            .pop_front()
            .ok_or_else(|| format!("unexpected {method} {url}"))?;
        if response.method != method || response.url != url {
            return Err(format!(
                "expected {} {}, got {method} {url}",
                response.method, response.url
            ));
        }
        Ok(response.body.to_string())
    }
}

impl LarkHttpClient for FakeClient {
    fn get(&self, url: &str, _tenant_access_token: &str) -> Result<String, String> {
        self.next("GET", url)
    }

    fn post_json(
        &self,
        url: &str,
        _body: &serde_json::Value,
        _tenant_access_token: Option<&str>,
    ) -> Result<String, String> {
        self.next("POST", url)
    }
}
