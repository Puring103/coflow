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

use coflow_api::{
    DataLoader, DataWriter, Diagnostic, DiagnosticSet, Label, LoadContext, LoadedRecords,
    LoaderDescriptor, ProbeResult, ProjectSourceRef, RecordOrigin, ResolvedSource, SourceDocument,
    SourceLocation, SourceLocationSpec, SourceResolveContext, WriteCellRequest, WriteContext,
    WriteFieldPathSegment, WriteOutcome, WriterCapabilities, WriterDescriptor,
};
use coflow_loader_table_core::cell_value::{render_cell_value, CellRenderError};
use coflow_loader_table_core::{
    collect_table_input_records, TableDiagnostic, TableDiagnostics, TableLabel, TableSheet,
    TableSheetConfig, TableSource,
};
use percent_encoding::{utf8_percent_encode, AsciiSet, CONTROLS};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};

const AUTH_URL: &str = "https://open.feishu.cn/open-apis/auth/v3/tenant_access_token/internal";
const API_BASE: &str = "https://open.feishu.cn/open-apis";
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LarkSheetSource {
    pub app_id: String,
    pub app_secret: String,
    pub locator: LarkSheetLocator,
    pub sheets: Vec<TableSheetConfig>,
}

impl LarkSheetSource {
    #[must_use]
    pub fn new(
        app_id: impl Into<String>,
        app_secret: impl Into<String>,
        locator: LarkSheetLocator,
        sheets: Vec<TableSheetConfig>,
    ) -> Self {
        Self {
            app_id: app_id.into(),
            app_secret: app_secret.into(),
            locator,
            sheets,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LarkSheetLocator {
    Url(String),
    SpreadsheetToken(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LarkDiagnostics {
    pub diagnostics: Vec<LarkDiagnostic>,
}

impl LarkDiagnostics {
    fn one(diagnostic: LarkDiagnostic) -> Self {
        Self {
            diagnostics: vec![diagnostic],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LarkDiagnostic {
    pub code: String,
    pub stage: String,
    pub message: String,
    pub document: Option<String>,
    pub sheet: Option<String>,
}

impl LarkDiagnostic {
    #[must_use]
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            stage: "LARK".to_string(),
            message: message.into(),
            document: None,
            sheet: None,
        }
    }

    #[must_use]
    pub fn with_document(mut self, document: impl Into<String>) -> Self {
        self.document = Some(document.into());
        self
    }

    #[must_use]
    pub fn with_sheet(mut self, sheet: impl Into<String>) -> Self {
        self.sheet = Some(sheet.into());
        self
    }
}

pub trait LarkHttpClient {
    /// Performs a Feishu/Lark authenticated GET request.
    ///
    /// # Errors
    ///
    /// Returns a transport or HTTP response error message.
    fn get(&self, url: &str, tenant_access_token: &str) -> Result<String, String>;

    /// Performs a Feishu/Lark JSON POST request.
    ///
    /// # Errors
    ///
    /// Returns a transport or HTTP response error message.
    fn post_json(
        &self,
        url: &str,
        body: &Value,
        tenant_access_token: Option<&str>,
    ) -> Result<String, String>;

    /// Performs an authenticated PUT request with a JSON body. Writers use
    /// this for batch update endpoints; the default implementation routes
    /// through `post_json` so existing fakes only need to implement two
    /// methods, but real clients should override for correct semantics.
    ///
    /// # Errors
    ///
    /// Returns a transport or HTTP response error message.
    fn put_json(
        &self,
        url: &str,
        body: &Value,
        tenant_access_token: &str,
    ) -> Result<String, String> {
        self.post_json(url, body, Some(tenant_access_token))
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct UreqLarkHttpClient;

impl LarkHttpClient for UreqLarkHttpClient {
    fn get(&self, url: &str, tenant_access_token: &str) -> Result<String, String> {
        ureq::get(url)
            .set("Authorization", &format!("Bearer {tenant_access_token}"))
            .call()
            .map_err(ureq_error_message)?
            .into_string()
            .map_err(|err| err.to_string())
    }

    fn post_json(
        &self,
        url: &str,
        body: &Value,
        tenant_access_token: Option<&str>,
    ) -> Result<String, String> {
        let mut request = ureq::post(url).set("Content-Type", "application/json");
        let bearer;
        if let Some(token) = tenant_access_token {
            bearer = format!("Bearer {token}");
            request = request.set("Authorization", &bearer);
        }
        request
            .send_string(&body.to_string())
            .map_err(ureq_error_message)?
            .into_string()
            .map_err(|err| err.to_string())
    }

    fn put_json(
        &self,
        url: &str,
        body: &Value,
        tenant_access_token: &str,
    ) -> Result<String, String> {
        ureq::put(url)
            .set("Content-Type", "application/json")
            .set("Authorization", &format!("Bearer {tenant_access_token}"))
            .send_string(&body.to_string())
            .map_err(ureq_error_message)?
            .into_string()
            .map_err(|err| err.to_string())
    }
}

/// Loads a Feishu/Lark spreadsheet into an Excel-like table source.
///
/// # Errors
///
/// Returns diagnostics when authentication, URL resolution, metadata loading,
/// value loading, or API response parsing fails.
pub fn load_lark_table_source(source: &LarkSheetSource) -> Result<TableSource, LarkDiagnostics> {
    load_lark_table_source_with_client(source, &UreqLarkHttpClient)
}

/// Loads a Feishu/Lark spreadsheet with an injected HTTP client.
///
/// # Errors
///
/// Returns diagnostics when authentication, URL resolution, metadata loading,
/// value loading, or API response parsing fails.
pub fn load_lark_table_source_with_client(
    source: &LarkSheetSource,
    client: &impl LarkHttpClient,
) -> Result<TableSource, LarkDiagnostics> {
    let tenant_access_token = tenant_access_token(client, source)?;
    let spreadsheet_token = spreadsheet_token(client, source, &tenant_access_token)?;
    let metadata = spreadsheet_metadata(client, &spreadsheet_token, &tenant_access_token)?;
    let configs = configured_sheets(source, &metadata);
    let mut diagnostics = Vec::new();
    let mut table_sheets = Vec::new();

    for config in &configs {
        let Some(sheet) = metadata
            .iter()
            .find(|sheet| sheet.title == config.sheet || sheet.sheet_id == config.sheet)
        else {
            diagnostics.push(
                LarkDiagnostic::new(
                    "LARK-SHEET",
                    format!(
                        "spreadsheet `{spreadsheet_token}` is missing sheet `{}`",
                        config.sheet
                    ),
                )
                .with_document(format!("lark:{spreadsheet_token}"))
                .with_sheet(config.sheet.clone()),
            );
            continue;
        };
        let rows = if sheet.row_count() == 0 || sheet.column_count() == 0 {
            Vec::new()
        } else {
            sheet_values(client, &spreadsheet_token, sheet, &tenant_access_token)?
        };
        table_sheets.push(TableSheet::new(sheet.title.clone(), rows));
    }

    if diagnostics.is_empty() {
        Ok(TableSource::remote(
            format!("lark:{spreadsheet_token}"),
            lark_document(source),
            table_sheets,
            configs,
        ))
    } else {
        Err(LarkDiagnostics { diagnostics })
    }
}

fn tenant_access_token(
    client: &impl LarkHttpClient,
    source: &LarkSheetSource,
) -> Result<String, LarkDiagnostics> {
    let body = json!({
        "app_id": source.app_id,
        "app_secret": source.app_secret,
    });
    let response = client
        .post_json(AUTH_URL, &body, None)
        .map_err(|message| LarkDiagnostics::one(LarkDiagnostic::new("LARK-AUTH", message)))?;
    let auth: AuthResponse = parse_response("LARK-AUTH", "tenant access token", &response)?;
    if auth.code != 0 {
        return Err(LarkDiagnostics::one(LarkDiagnostic::new(
            "LARK-AUTH",
            api_error_message("tenant access token", auth.code, auth.msg.as_deref()),
        )));
    }
    auth.tenant_access_token.ok_or_else(|| {
        LarkDiagnostics::one(LarkDiagnostic::new(
            "LARK-AUTH",
            "tenant access token response did not include `tenant_access_token`",
        ))
    })
}

fn spreadsheet_token(
    client: &impl LarkHttpClient,
    source: &LarkSheetSource,
    tenant_access_token: &str,
) -> Result<String, LarkDiagnostics> {
    match &source.locator {
        LarkSheetLocator::SpreadsheetToken(token) => Ok(token.trim().to_string()),
        LarkSheetLocator::Url(url) => spreadsheet_token_from_url(client, url, tenant_access_token),
    }
}

fn spreadsheet_token_from_url(
    client: &impl LarkHttpClient,
    url: &str,
    tenant_access_token: &str,
) -> Result<String, LarkDiagnostics> {
    if let Some(token) = token_after_path_marker(url, "/sheets/") {
        return Ok(token);
    }
    let Some(wiki_token) = token_after_path_marker(url, "/wiki/") else {
        return Err(LarkDiagnostics::one(
            LarkDiagnostic::new(
                "LARK-URL",
                "lark source url must be a `/sheets/<token>` or `/wiki/<token>` URL",
            )
            .with_document(url.to_string()),
        ));
    };
    let endpoint = format!(
        "{API_BASE}/wiki/v2/spaces/get_node?token={}",
        url_component(&wiki_token)
    );
    let response = client
        .get(&endpoint, tenant_access_token)
        .map_err(|message| LarkDiagnostics::one(LarkDiagnostic::new("LARK-WIKI", message)))?;
    let envelope: ApiEnvelope<WikiNodeData> = parse_response("LARK-WIKI", "wiki node", &response)?;
    let data = envelope_data(envelope, "LARK-WIKI", "wiki node")?;
    if data.node.obj_type != "sheet" {
        return Err(LarkDiagnostics::one(
            LarkDiagnostic::new(
                "LARK-WIKI",
                format!(
                    "wiki node `{wiki_token}` points to `{}`, expected `sheet`",
                    data.node.obj_type
                ),
            )
            .with_document(url.to_string()),
        ));
    }
    Ok(data.node.obj_token)
}

fn spreadsheet_metadata(
    client: &impl LarkHttpClient,
    spreadsheet_token: &str,
    tenant_access_token: &str,
) -> Result<Vec<LarkSheetMetadata>, LarkDiagnostics> {
    let endpoint = format!(
        "{API_BASE}/sheets/v3/spreadsheets/{}/sheets/query",
        url_component(spreadsheet_token)
    );
    let response = client
        .get(&endpoint, tenant_access_token)
        .map_err(|message| LarkDiagnostics::one(LarkDiagnostic::new("LARK-SHEET", message)))?;
    let envelope: ApiEnvelope<SheetsQueryData> =
        parse_response("LARK-SHEET", "spreadsheet sheets", &response)?;
    Ok(envelope_data(envelope, "LARK-SHEET", "spreadsheet sheets")?.sheets)
}

fn configured_sheets(
    source: &LarkSheetSource,
    metadata: &[LarkSheetMetadata],
) -> Vec<TableSheetConfig> {
    if source.sheets.is_empty() {
        metadata
            .iter()
            .map(|sheet| TableSheetConfig::new(sheet.title.clone()))
            .collect()
    } else {
        source.sheets.clone()
    }
}

fn sheet_values(
    client: &impl LarkHttpClient,
    spreadsheet_token: &str,
    sheet: &LarkSheetMetadata,
    tenant_access_token: &str,
) -> Result<Vec<Vec<String>>, LarkDiagnostics> {
    let last_column = column_name(sheet.column_count());
    let range = format!("{}!A1:{last_column}{}", sheet.sheet_id, sheet.row_count());
    let endpoint = format!(
        "{API_BASE}/sheets/v2/spreadsheets/{}/values/{}?valueRenderOption=ToString",
        url_component(spreadsheet_token),
        url_component(&range)
    );
    let response = client
        .get(&endpoint, tenant_access_token)
        .map_err(|message| {
            LarkDiagnostics::one(
                LarkDiagnostic::new("LARK-VALUE", message)
                    .with_document(format!("lark:{spreadsheet_token}"))
                    .with_sheet(sheet.title.clone()),
            )
        })?;
    let envelope: ApiEnvelope<ValuesData> =
        parse_response("LARK-VALUE", "spreadsheet values", &response)?;
    let data = envelope_data(envelope, "LARK-VALUE", "spreadsheet values")?;
    Ok(data.value_range.values.into_iter().map(json_row).collect())
}

fn json_row(row: Vec<Value>) -> Vec<String> {
    row.into_iter().map(json_cell_text).collect()
}

fn json_cell_text(value: Value) -> String {
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

fn token_after_path_marker(url: &str, marker: &str) -> Option<String> {
    let marker_start = url.find(marker)?;
    let token_start = marker_start + marker.len();
    let rest = &url[token_start..];
    let token = rest
        .split(['?', '#', '/'])
        .next()
        .unwrap_or_default()
        .trim();
    (!token.is_empty()).then(|| token.to_string())
}

fn column_name(column: usize) -> String {
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

fn url_component(value: &str) -> String {
    utf8_percent_encode(value, URL_COMPONENT_ENCODE_SET).to_string()
}

fn parse_response<T: DeserializeOwned>(
    code: &str,
    description: &str,
    response: &str,
) -> Result<T, LarkDiagnostics> {
    serde_json::from_str(response).map_err(|err| {
        LarkDiagnostics::one(LarkDiagnostic::new(
            code,
            format!("failed to parse {description} response: {err}"),
        ))
    })
}

fn envelope_data<T>(
    envelope: ApiEnvelope<T>,
    code: &str,
    description: &str,
) -> Result<T, LarkDiagnostics> {
    if envelope.code != 0 {
        return Err(LarkDiagnostics::one(LarkDiagnostic::new(
            code,
            api_error_message(description, envelope.code, envelope.msg.as_deref()),
        )));
    }
    envelope.data.ok_or_else(|| {
        LarkDiagnostics::one(LarkDiagnostic::new(
            code,
            format!("{description} response did not include `data`"),
        ))
    })
}

fn api_error_message(description: &str, code: i64, msg: Option<&str>) -> String {
    msg.map_or_else(
        || format!("{description} API returned code {code}"),
        |message| format!("{description} API returned code {code}: {message}"),
    )
}

fn ureq_error_message(err: ureq::Error) -> String {
    match err {
        ureq::Error::Status(code, response) => {
            let status = response.status_text().to_string();
            match response.into_string() {
                Ok(body) if !body.is_empty() => {
                    format!("HTTP {code} {status}: {body}")
                }
                _ => format!("HTTP {code} {status}"),
            }
        }
        ureq::Error::Transport(err) => err.to_string(),
    }
}

#[derive(Debug, Clone)]
pub struct LarkSheetLoader<C = UreqLarkHttpClient> {
    client: C,
}

impl Default for LarkSheetLoader<UreqLarkHttpClient> {
    fn default() -> Self {
        Self {
            client: UreqLarkHttpClient,
        }
    }
}

impl<C> LarkSheetLoader<C> {
    #[must_use]
    pub fn new(client: C) -> Self {
        Self { client }
    }
}

pub const LARK_SHEET_LOADER_DESCRIPTOR: LoaderDescriptor = LoaderDescriptor {
    id: "lark-sheet",
    display_name: "Lark Sheet",
    extensions: &[],
    uri_schemes: &["https", "lark"],
    option_keys: &["spreadsheet_token", "url", "app_id", "app_secret"],
};

impl<C> DataLoader for LarkSheetLoader<C>
where
    C: LarkHttpClient + Send + Sync,
{
    fn descriptor(&self) -> &'static LoaderDescriptor {
        &LARK_SHEET_LOADER_DESCRIPTOR
    }

    fn probe(&self, source: &ProjectSourceRef<'_>) -> ProbeResult {
        if source.source_type == Some(LARK_SHEET_LOADER_DESCRIPTOR.id) {
            return ProbeResult::certain();
        }
        if let SourceLocationSpec::Uri(uri) = source.location {
            if source
                .option_keys
                .iter()
                .any(|key| LARK_SHEET_LOADER_DESCRIPTOR.option_keys.contains(key))
            {
                return ProbeResult::certain();
            }
            if is_lark_uri(uri) {
                return ProbeResult::likely();
            }
        }
        ProbeResult::none()
    }

    fn resolve(
        &self,
        _ctx: SourceResolveContext<'_>,
        source: &ResolvedSource,
    ) -> Result<Vec<ResolvedSource>, DiagnosticSet> {
        let SourceLocationSpec::Uri(uri) = &source.location else {
            if source.provider_id == LARK_SHEET_LOADER_DESCRIPTOR.id {
                return Err(DiagnosticSet::one(Diagnostic::error(
                    "LARK-SOURCE",
                    "LARK",
                    "lark source requires `url`",
                )));
            }
            return Ok(Vec::new());
        };
        if !is_lark_uri(uri) {
            return Err(DiagnosticSet::one(Diagnostic::error(
                "LARK-SOURCE",
                "LARK",
                "lark source url must be an `https://` Feishu/Lark URL or `lark:<spreadsheet_token>`",
            )));
        }
        let mut resolved = source.clone();
        resolved.provider_id = LARK_SHEET_LOADER_DESCRIPTOR.id.to_string();
        Ok(vec![resolved])
    }

    fn load(
        &self,
        ctx: LoadContext<'_>,
        source: &ResolvedSource,
    ) -> Result<LoadedRecords, DiagnosticSet> {
        let lark_source = lark_source_from_spec(source)?;
        let table_source = load_lark_table_source_with_client(&lark_source, &self.client)
            .map_err(lark_diagnostics_to_api)?;
        collect_table_input_records(ctx.schema, &[table_source])
            .map(|loaded| LoadedRecords {
                records: loaded.records,
            })
            .map_err(table_diagnostics_to_api)
    }
}

fn lark_source_from_spec(source: &ResolvedSource) -> Result<LarkSheetSource, DiagnosticSet> {
    let options = &source.options;
    let app_id = required_option_string(options, "app_id")?;
    let app_secret = required_option_string(options, "app_secret")?;
    let source_url = match &source.location {
        SourceLocationSpec::Uri(uri) => Some(uri.clone()),
        SourceLocationSpec::Path(_) => None,
    };
    let url = option_string(options, "url").or_else(|| source_url.clone());
    let spreadsheet_token = option_string(options, "spreadsheet_token").or_else(|| {
        source_url
            .as_deref()
            .and_then(lark_token_uri)
            .map(str::to_string)
    });
    let locator = match (url, spreadsheet_token) {
        (Some(url), None) => LarkSheetLocator::Url(url),
        (Some(url), Some(token))
            if lark_token_uri(&url).is_some_and(|uri_token| uri_token == token) =>
        {
            LarkSheetLocator::SpreadsheetToken(token)
        }
        (None, Some(token)) => LarkSheetLocator::SpreadsheetToken(token),
        (Some(_), Some(_)) => {
            return Err(DiagnosticSet::one(Diagnostic::error(
                "LARK-SOURCE",
                "LARK",
                "lark source must set exactly one of `url` or `spreadsheet_token`",
            )))
        }
        (None, None) => {
            return Err(DiagnosticSet::one(Diagnostic::error(
                "LARK-SOURCE",
                "LARK",
                "lark source requires `url` or `spreadsheet_token`",
            )))
        }
    };
    Ok(LarkSheetSource::new(
        app_id,
        app_secret,
        locator,
        table_sheet_configs_from_options(options)?,
    ))
}

fn is_lark_uri(uri: &str) -> bool {
    lark_token_uri(uri).is_some()
        || (uri.starts_with("https://") && (uri.contains("feishu") || uri.contains("larksuite")))
}

fn lark_token_uri(uri: &str) -> Option<&str> {
    let token = uri.strip_prefix("lark:")?;
    (!token.trim().is_empty()).then_some(token)
}

fn required_option_string(options: &Value, key: &str) -> Result<String, DiagnosticSet> {
    option_string(options, key).ok_or_else(|| {
        DiagnosticSet::one(Diagnostic::error(
            "LARK-SOURCE",
            "LARK",
            format!("lark source requires `{key}`"),
        ))
    })
}

fn option_string(options: &Value, key: &str) -> Option<String> {
    options.get(key).and_then(Value::as_str).map(str::to_string)
}

fn table_sheet_configs_from_options(
    options: &Value,
) -> Result<Vec<TableSheetConfig>, DiagnosticSet> {
    let Some(sheets) = options.get("sheets") else {
        return Ok(Vec::new());
    };
    let Some(sheets) = sheets.as_array() else {
        return Err(DiagnosticSet::one(Diagnostic::error(
            "LARK-SOURCE",
            "LARK",
            "lark source option `sheets` must be an array",
        )));
    };
    sheets
        .iter()
        .map(table_sheet_config_from_value)
        .collect::<Result<Vec<_>, _>>()
}

fn table_sheet_config_from_value(value: &Value) -> Result<TableSheetConfig, DiagnosticSet> {
    let Some(object) = value.as_object() else {
        return Err(DiagnosticSet::one(Diagnostic::error(
            "LARK-SOURCE",
            "LARK",
            "lark source sheet config must be an object",
        )));
    };
    let Some(sheet_name) = object.get("sheet").and_then(Value::as_str) else {
        return Err(DiagnosticSet::one(Diagnostic::error(
            "LARK-SOURCE",
            "LARK",
            "lark source sheet config requires `sheet`",
        )));
    };
    if sheet_name.trim().is_empty() {
        return Err(DiagnosticSet::one(Diagnostic::error(
            "LARK-SOURCE",
            "LARK",
            "lark source sheet `sheet` is empty",
        )));
    }
    let mut sheet = TableSheetConfig::new(sheet_name);
    if let Some(type_name) = optional_string_field(object, "type", "lark source sheet `type`")? {
        if type_name.trim().is_empty() {
            return Err(DiagnosticSet::one(Diagnostic::error(
                "LARK-SOURCE",
                "LARK",
                "lark source sheet `type` is empty",
            )));
        }
        sheet = sheet.with_type(type_name);
    }
    if let Some(key) = optional_string_field(object, "key", "lark source sheet `key`")? {
        if key.trim().is_empty() {
            return Err(DiagnosticSet::one(Diagnostic::error(
                "LARK-SOURCE",
                "LARK",
                "lark source sheet `key` is empty",
            )));
        }
        sheet = sheet.with_key(key);
    }
    if let Some(columns) = object.get("columns") {
        let Some(columns) = columns.as_object() else {
            return Err(DiagnosticSet::one(Diagnostic::error(
                "LARK-SOURCE",
                "LARK",
                "lark source sheet `columns` must be an object",
            )));
        };
        let mut parsed_columns = Vec::new();
        for (source, field) in columns {
            let Some(field) = field.as_str() else {
                return Err(DiagnosticSet::one(Diagnostic::error(
                    "LARK-SOURCE",
                    "LARK",
                    format!("lark source sheet column `{source}` must map to a string field"),
                )));
            };
            if source.trim().is_empty() {
                return Err(DiagnosticSet::one(Diagnostic::error(
                    "LARK-SOURCE",
                    "LARK",
                    "lark source sheet column name is empty",
                )));
            }
            if field.trim().is_empty() {
                return Err(DiagnosticSet::one(Diagnostic::error(
                    "LARK-SOURCE",
                    "LARK",
                    format!("lark source sheet column `{source}` maps to an empty field"),
                )));
            }
            parsed_columns.push((source.as_str(), field));
        }
        sheet = sheet.with_columns(parsed_columns);
    }
    Ok(sheet)
}

fn lark_document(source: &LarkSheetSource) -> String {
    match &source.locator {
        LarkSheetLocator::Url(url) => url.clone(),
        LarkSheetLocator::SpreadsheetToken(token) => format!("lark:{token}"),
    }
}

fn optional_string_field<'a>(
    object: &'a serde_json::Map<String, Value>,
    key: &str,
    label: &str,
) -> Result<Option<&'a str>, DiagnosticSet> {
    let Some(value) = object.get(key) else {
        return Ok(None);
    };
    value.as_str().map(Some).ok_or_else(|| {
        DiagnosticSet::one(Diagnostic::error(
            "LARK-SOURCE",
            "LARK",
            format!("{label} must be a string"),
        ))
    })
}

fn lark_diagnostics_to_api(err: LarkDiagnostics) -> DiagnosticSet {
    DiagnosticSet {
        diagnostics: err
            .diagnostics
            .into_iter()
            .map(lark_diagnostic_to_api)
            .collect(),
    }
}

fn lark_diagnostic_to_api(diagnostic: LarkDiagnostic) -> Diagnostic {
    let document = diagnostic.document.clone().unwrap_or_default();
    Diagnostic {
        code: diagnostic.code,
        stage: diagnostic.stage,
        severity: coflow_api::Severity::Error,
        message: diagnostic.message,
        primary: Some(Label {
            location: SourceLocation::RemoteCell {
                document,
                sheet: diagnostic.sheet,
                row: 0,
                column: 0,
            },
            message: None,
        }),
        related: Vec::new(),
    }
}

fn table_diagnostics_to_api(err: TableDiagnostics) -> DiagnosticSet {
    DiagnosticSet {
        diagnostics: err
            .diagnostics
            .into_iter()
            .map(table_diagnostic_to_api)
            .collect(),
    }
}

fn table_diagnostic_to_api(diagnostic: TableDiagnostic) -> Diagnostic {
    Diagnostic {
        code: diagnostic.code,
        stage: diagnostic.stage,
        severity: coflow_api::Severity::Error,
        message: diagnostic.message,
        primary: diagnostic.primary.map(table_label_to_api),
        related: diagnostic
            .related
            .into_iter()
            .map(table_label_to_api)
            .collect(),
    }
}

fn table_label_to_api(label: TableLabel) -> Label {
    Label {
        location: coflow_data_model::SourceLocation::from(label.location).into(),
        message: label.message,
    }
}

/// Writer descriptor for Lark sheets. Capabilities expose this as a remote,
/// field-edit-only writer (no record insertion via this writer yet).
pub const LARK_SHEET_WRITER_DESCRIPTOR: WriterDescriptor = WriterDescriptor {
    id: "lark-sheet",
    display_name: "Lark Sheet",
    capabilities: WriterCapabilities::remote_field_edit(),
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
        let spreadsheet_token = doc
            .strip_prefix("lark:")
            .ok_or_else(|| {
                DiagnosticSet::one(diag(
                    "LARK-WRITE",
                    format!("unsupported lark document: `{doc}`"),
                ))
            })?
            .to_string();

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
        let envelope: ApiEnvelope<Value> = serde_json::from_str(&response).map_err(|err| {
            LarkWriteFailure::Other(DiagnosticSet::one(diag(
                "LARK-WRITE",
                format!("failed to parse values_batch_update response: {err}"),
            )))
        })?;
        if envelope.code == 0 {
            return Ok(());
        }
        let diag_set = DiagnosticSet::one(diag(
            "LARK-WRITE",
            api_error_message(
                "values_batch_update",
                envelope.code,
                envelope.msg.as_deref(),
            ),
        ));
        // Lark's "tenant access token invalid / expired" family hovers
        // around 99991663 / 99991668. Treat any 9999xxxx code as a hint
        // that the token has gone stale and let the writer retry once.
        if (99_991_000..100_000_000).contains(&envelope.code) {
            Err(LarkWriteFailure::TokenExpired(diag_set))
        } else {
            Err(LarkWriteFailure::Other(diag_set))
        }
    }
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

fn diag(code: &'static str, message: impl Into<String>) -> Diagnostic {
    Diagnostic::error(code, "LARK", message)
}

fn lark_render_error(err: CellRenderError) -> DiagnosticSet {
    let message = match err {
        CellRenderError::AnonymousEnum => {
            "writing anonymous enum values into lark cells is not supported"
        }
        CellRenderError::NestedObject => {
            "writing nested object values into lark cells is not supported"
        }
    };
    DiagnosticSet::one(diag("LARK-WRITE", message))
}

#[derive(Debug, Deserialize)]
struct AuthResponse {
    code: i64,
    msg: Option<String>,
    tenant_access_token: Option<String>,
    /// Server-declared TTL in seconds. Lark documents 7200 today; callers
    /// nonetheless treat this as advisory and apply a safety margin before
    /// reuse.
    #[serde(default)]
    expire: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct ApiEnvelope<T> {
    code: i64,
    msg: Option<String>,
    data: Option<T>,
}

#[derive(Debug, Deserialize)]
struct WikiNodeData {
    node: WikiNode,
}

#[derive(Debug, Deserialize)]
struct WikiNode {
    obj_type: String,
    obj_token: String,
}

#[derive(Debug, Deserialize)]
struct SheetsQueryData {
    sheets: Vec<LarkSheetMetadata>,
}

#[derive(Debug, Clone, Deserialize)]
struct LarkSheetMetadata {
    sheet_id: String,
    title: String,
    #[serde(default, flatten)]
    grid: GridContainer,
}

impl LarkSheetMetadata {
    fn row_count(&self) -> usize {
        self.grid
            .grid_properties
            .as_ref()
            .map_or(0, |grid| grid.row_count)
    }

    fn column_count(&self) -> usize {
        self.grid
            .grid_properties
            .as_ref()
            .map_or(0, |grid| grid.column_count)
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
struct GridContainer {
    grid_properties: Option<GridProperties>,
}

#[derive(Debug, Clone, Deserialize)]
struct GridProperties {
    #[serde(default)]
    row_count: usize,
    #[serde(default)]
    column_count: usize,
}

#[derive(Debug, Deserialize)]
struct ValuesData {
    #[serde(rename = "valueRange", alias = "value_range")]
    value_range: ValueRange,
}

#[derive(Debug, Deserialize)]
struct ValueRange {
    #[serde(default)]
    values: Vec<Vec<Value>>,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::panic)]

    use super::*;
    use coflow_api::{CftContainer, SourceResolveContext};
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
}
