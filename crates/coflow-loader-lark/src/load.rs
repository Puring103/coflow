use coflow_api::{
    DecodedSourceOptions, Diagnostic, DiagnosticSet, LoadedSource, ProbeResult, ProjectSourceRef,
    ResolvedSource, SourceLoadContext, SourceLocationSpec, SourceProvider,
    SourceProviderDescriptor, SourceResolveContext,
};
use coflow_loader_table_core::{
    collect_table_input_records, TableSheet, TableSheetConfig, TableSource,
};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};

use crate::diagnostics::{lark_diagnostics_to_api, table_diagnostics_to_api};
use crate::dto::{
    ApiEnvelope, AuthResponse, LarkSheetMetadata, SheetsQueryData, ValuesData, WikiNodeData,
};
use crate::http::{LarkHttpClient, UreqLarkHttpClient};
use crate::source::{
    decode_lark_source_options, is_lark_uri, lark_document, lark_source_from_spec,
    token_after_path_marker, LarkSheetLocator, LarkSheetSource,
};
use crate::{
    api_error_message, column_name, json_cell_text, url_component, LarkDiagnostic, LarkDiagnostics,
    API_BASE, AUTH_URL,
};

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
    build_table_source(
        client,
        source,
        &spreadsheet_token,
        &tenant_access_token,
        &metadata,
    )
}

fn build_table_source(
    client: &impl LarkHttpClient,
    source: &LarkSheetSource,
    spreadsheet_token: &str,
    tenant_access_token: &str,
    metadata: &[LarkSheetMetadata],
) -> Result<TableSource, LarkDiagnostics> {
    let configs = configured_sheets(source, metadata);
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
            sheet_values(client, spreadsheet_token, sheet, tenant_access_token)?
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

pub(crate) fn spreadsheet_token(
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

#[derive(Debug, Clone)]
pub struct LarkSheetLoader<C = UreqLarkHttpClient> {
    client: C,
    cache: std::sync::Arc<std::sync::Mutex<LarkLoaderCache>>,
}

#[derive(Debug, Default)]
struct LarkLoaderCache {
    tokens: std::collections::HashMap<String, LoaderCachedToken>,
    spreadsheet_tokens: std::collections::HashMap<String, String>,
    metadata: std::collections::HashMap<String, Vec<LarkSheetMetadata>>,
}

#[derive(Debug, Clone)]
struct LoaderCachedToken {
    token: String,
    expires_at: std::time::Instant,
}

impl Default for LarkSheetLoader<UreqLarkHttpClient> {
    fn default() -> Self {
        Self {
            client: UreqLarkHttpClient,
            cache: std::sync::Arc::new(std::sync::Mutex::new(LarkLoaderCache::default())),
        }
    }
}

impl<C> LarkSheetLoader<C> {
    #[must_use]
    pub fn new(client: C) -> Self {
        Self {
            client,
            cache: std::sync::Arc::new(std::sync::Mutex::new(LarkLoaderCache::default())),
        }
    }
}

impl<C> LarkSheetLoader<C>
where
    C: LarkHttpClient + Send + Sync,
{
    fn load_lark_table_source_cached(
        &self,
        source: &LarkSheetSource,
    ) -> Result<TableSource, LarkDiagnostics> {
        let tenant_access_token = self.cached_loader_tenant_token(source)?;
        let spreadsheet_token =
            self.cached_loader_spreadsheet_token(source, &tenant_access_token)?;
        let metadata = self.cached_loader_metadata(&spreadsheet_token, &tenant_access_token)?;
        build_table_source(
            &self.client,
            source,
            &spreadsheet_token,
            &tenant_access_token,
            &metadata,
        )
    }

    fn cached_loader_tenant_token(
        &self,
        source: &LarkSheetSource,
    ) -> Result<String, LarkDiagnostics> {
        let now = std::time::Instant::now();
        if let Ok(cache) = self.cache.lock() {
            if let Some(entry) = cache.tokens.get(&source.app_id) {
                if entry.expires_at > now {
                    return Ok(entry.token.clone());
                }
            }
        }
        let token = tenant_access_token(&self.client, source)?;
        let expires_at = now + std::time::Duration::from_mins(30);
        if let Ok(mut cache) = self.cache.lock() {
            cache.tokens.insert(
                source.app_id.clone(),
                LoaderCachedToken {
                    token: token.clone(),
                    expires_at,
                },
            );
        }
        Ok(token)
    }

    fn cached_loader_spreadsheet_token(
        &self,
        source: &LarkSheetSource,
        tenant_access_token: &str,
    ) -> Result<String, LarkDiagnostics> {
        let cache_key = match &source.locator {
            LarkSheetLocator::SpreadsheetToken(token) => return Ok(token.trim().to_string()),
            LarkSheetLocator::Url(url) => url.clone(),
        };
        if let Ok(cache) = self.cache.lock() {
            if let Some(token) = cache.spreadsheet_tokens.get(&cache_key) {
                return Ok(token.clone());
            }
        }
        let token = spreadsheet_token(&self.client, source, tenant_access_token)?;
        if let Ok(mut cache) = self.cache.lock() {
            cache.spreadsheet_tokens.insert(cache_key, token.clone());
        }
        Ok(token)
    }

    fn cached_loader_metadata(
        &self,
        spreadsheet_token: &str,
        tenant_access_token: &str,
    ) -> Result<Vec<LarkSheetMetadata>, LarkDiagnostics> {
        if let Ok(cache) = self.cache.lock() {
            if let Some(metadata) = cache.metadata.get(spreadsheet_token) {
                return Ok(metadata.clone());
            }
        }
        let metadata = spreadsheet_metadata(&self.client, spreadsheet_token, tenant_access_token)?;
        if let Ok(mut cache) = self.cache.lock() {
            cache
                .metadata
                .insert(spreadsheet_token.to_string(), metadata.clone());
        }
        Ok(metadata)
    }
}

pub const LARK_SHEET_LOADER_DESCRIPTOR: SourceProviderDescriptor = SourceProviderDescriptor {
    id: "lark-sheet",
    display_name: "Lark Sheet",
    extensions: &[],
    uri_schemes: &["https", "lark"],
    option_keys: &["spreadsheet_token", "url", "app_id", "app_secret"],
};

impl<C> SourceProvider for LarkSheetLoader<C>
where
    C: LarkHttpClient + Send + Sync,
{
    fn descriptor(&self) -> &'static SourceProviderDescriptor {
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

    fn decode_options(&self, options: &Value) -> Result<DecodedSourceOptions, DiagnosticSet> {
        decode_lark_source_options(options)
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
        ctx: SourceLoadContext<'_>,
        source: &ResolvedSource,
    ) -> Result<LoadedSource, DiagnosticSet> {
        let lark_source = lark_source_from_spec(source)?;
        let table_source = self
            .load_lark_table_source_cached(&lark_source)
            .map_err(lark_diagnostics_to_api)?;
        collect_table_input_records(ctx.schema, &[table_source])
            .map(|loaded| LoadedSource {
                records: loaded.records,
            })
            .map_err(table_diagnostics_to_api)
    }
}
