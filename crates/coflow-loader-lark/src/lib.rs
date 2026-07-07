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
mod write;
mod writer_cache;

use percent_encoding::{utf8_percent_encode, AsciiSet, CONTROLS};
use serde_json::Value;

use writer_cache::LarkWriterCache;

pub use diagnostics::{LarkDiagnostic, LarkDiagnostics};
pub use http::{LarkHttpClient, UreqLarkHttpClient};
pub use load::{
    load_lark_table_source, load_lark_table_source_with_client, LarkSheetLoader,
    LARK_SHEET_LOADER_DESCRIPTOR,
};
pub use source::{LarkSheetLocator, LarkSheetSource};
pub use write::LARK_SHEET_WRITER_DESCRIPTOR;

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

#[cfg(test)]
mod tests {
    #![allow(clippy::panic)]

    use super::*;
    use coflow_api::{
        CftContainer, DataLoader, LoadContext, ModuleId, ProbeResult, ProjectSourceRef,
        ResolvedSource, SourceLocationSpec, SourceResolveContext,
    };
    use crate::source::lark_source_from_spec;
    use serde_json::{json, Value};
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
