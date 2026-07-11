use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::diagnostics::lark_diagnostics_to_api;
use crate::dto::{ApiEnvelope, AuthResponse, LarkSheetMetadata, SheetsQueryData, WikiNodeData};
use crate::http::{LarkHttpClient, UreqLarkHttpClient};
use crate::source::{
    lark_source_from_spec, token_after_path_marker, LarkSheetLocator, LarkSheetSource,
};
use crate::{
    api_error_message, url_component, LarkDiagnostic, LarkDiagnostics, LarkSheetLoader,
    LarkSheetWriter, API_BASE, AUTH_URL,
};
use coflow_api::{DiagnosticSet, ResolvedSource};

pub(crate) struct LarkRemote<C> {
    client: C,
    state: Mutex<LarkRemoteState>,
}

impl<C> std::fmt::Debug for LarkRemote<C> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("LarkRemote").finish_non_exhaustive()
    }
}

#[derive(Debug, Default)]
struct LarkRemoteState {
    tokens: HashMap<CredentialKey, CachedToken>,
    spreadsheet_tokens: HashMap<(CredentialKey, String), String>,
    sheets: HashMap<(CredentialKey, String), CachedSheets>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct CredentialKey {
    app_id: String,
    secret_fingerprint: [u8; 32],
}

impl CredentialKey {
    fn new(app_id: &str, app_secret: &str) -> Self {
        Self {
            app_id: app_id.to_string(),
            secret_fingerprint: Sha256::digest(app_secret.as_bytes()).into(),
        }
    }
}

#[derive(Debug, Clone)]
struct CachedToken {
    token: String,
    expires_at: std::time::Instant,
}

#[derive(Debug, Clone)]
struct CachedSheets {
    ordered: Vec<LarkSheetMetadata>,
    by_name_or_id: HashMap<String, LarkSheetMetadata>,
}

impl CachedSheets {
    fn new(ordered: Vec<LarkSheetMetadata>) -> Self {
        let mut by_name_or_id = HashMap::new();
        for sheet in &ordered {
            by_name_or_id.insert(sheet.title.clone(), sheet.clone());
            by_name_or_id.insert(sheet.sheet_id.clone(), sheet.clone());
        }
        Self {
            ordered,
            by_name_or_id,
        }
    }
}

#[derive(Clone)]
pub(crate) struct LarkAuth {
    key: CredentialKey,
    app_secret: String,
    pub(crate) token: String,
}

impl std::fmt::Debug for LarkAuth {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LarkAuth")
            .field("app_id", &self.key.app_id)
            .field("app_secret", &"[redacted]")
            .field("token", &"[redacted]")
            .finish()
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum LarkHttpMethod {
    Get,
    Post,
    Put,
    Delete,
}

impl LarkHttpMethod {
    const fn name(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Delete => "DELETE",
        }
    }
}

pub(crate) struct LarkRequest<'a> {
    pub(crate) method: LarkHttpMethod,
    pub(crate) code: &'static str,
    pub(crate) description: &'static str,
    pub(crate) endpoint: &'a str,
    pub(crate) body: Option<&'a Value>,
}

impl<C> LarkRemote<C> {
    pub(crate) fn new(client: C) -> Self {
        Self {
            client,
            state: Mutex::new(LarkRemoteState::default()),
        }
    }
}

impl<C> LarkRemote<C>
where
    C: LarkHttpClient,
{
    pub(crate) fn authenticate(
        &self,
        app_id: &str,
        app_secret: &str,
    ) -> Result<LarkAuth, LarkDiagnostics> {
        let key = CredentialKey::new(app_id, app_secret);
        let now = std::time::Instant::now();
        if let Ok(state) = self.state.lock() {
            if let Some(entry) = state.tokens.get(&key) {
                if entry.expires_at > now {
                    return Ok(LarkAuth {
                        key,
                        app_secret: app_secret.to_string(),
                        token: entry.token.clone(),
                    });
                }
            }
        }

        let (token, ttl_secs) = self.fetch_tenant_token(app_id, app_secret)?;
        let safety_margin = std::time::Duration::from_mins(1);
        let lifetime = ttl_secs.map_or_else(
            || std::time::Duration::from_mins(30),
            std::time::Duration::from_secs,
        );
        let expires_at = now + lifetime.saturating_sub(safety_margin);
        if let Ok(mut state) = self.state.lock() {
            state.tokens.insert(
                key.clone(),
                CachedToken {
                    token: token.clone(),
                    expires_at,
                },
            );
        }
        Ok(LarkAuth {
            key,
            app_secret: app_secret.to_string(),
            token,
        })
    }

    fn fetch_tenant_token(
        &self,
        app_id: &str,
        app_secret: &str,
    ) -> Result<(String, Option<u64>), LarkDiagnostics> {
        let body = json!({ "app_id": app_id, "app_secret": app_secret });
        let response = self
            .client
            .post_json(AUTH_URL, &body, None)
            .map_err(|message| remote_error("LARK-AUTH", "POST", "tenant access token", message))?;
        let envelope: AuthResponse = parse_response("LARK-AUTH", "tenant access token", &response)?;
        if envelope.code != 0 {
            return Err(LarkDiagnostics::one(LarkDiagnostic::new(
                "LARK-AUTH",
                api_error_message(
                    "tenant access token",
                    envelope.code,
                    envelope.msg.as_deref(),
                ),
            )));
        }
        let token = envelope.tenant_access_token.ok_or_else(|| {
            LarkDiagnostics::one(LarkDiagnostic::new(
                "LARK-AUTH",
                "tenant access token response did not include `tenant_access_token`",
            ))
        })?;
        Ok((token, envelope.expire))
    }

    pub(crate) fn request(
        &self,
        auth: &LarkAuth,
        request: &LarkRequest<'_>,
    ) -> Result<String, LarkDiagnostics> {
        let active = self.authenticate(&auth.key.app_id, &auth.app_secret)?;
        let response = self.request_once(&active, request, false)?;
        if !token_expired(&response) {
            return Ok(response);
        }

        self.invalidate_auth(&active);
        let fresh = self.authenticate(&auth.key.app_id, &auth.app_secret)?;
        self.request_once(&fresh, request, true)
    }

    fn request_once(
        &self,
        auth: &LarkAuth,
        request: &LarkRequest<'_>,
        retry: bool,
    ) -> Result<String, LarkDiagnostics> {
        let result = match request.method {
            LarkHttpMethod::Get => self.client.get(request.endpoint, &auth.token),
            LarkHttpMethod::Post => self.client.post_json(
                request.endpoint,
                request.body.unwrap_or(&Value::Null),
                Some(&auth.token),
            ),
            LarkHttpMethod::Put => self.client.put_json(
                request.endpoint,
                request.body.unwrap_or(&Value::Null),
                &auth.token,
            ),
            LarkHttpMethod::Delete => self.client.delete_json(
                request.endpoint,
                request.body.unwrap_or(&Value::Null),
                &auth.token,
            ),
        };
        result.map_err(|message| {
            let description = if retry {
                format!("{} retry after token refresh", request.description)
            } else {
                request.description.to_string()
            };
            remote_error(request.code, request.method.name(), &description, message)
        })
    }

    fn invalidate_auth(&self, auth: &LarkAuth) {
        if let Ok(mut state) = self.state.lock() {
            if state
                .tokens
                .get(&auth.key)
                .is_some_and(|cached| cached.token == auth.token)
            {
                state.tokens.remove(&auth.key);
            }
        }
    }

    pub(crate) fn invalidate_document(&self, auth: &LarkAuth, spreadsheet_token: &str) {
        if let Ok(mut state) = self.state.lock() {
            state
                .sheets
                .remove(&(auth.key.clone(), spreadsheet_token.to_string()));
        }
    }

    pub(crate) fn spreadsheet_token(
        &self,
        source: &LarkSheetSource,
        auth: &LarkAuth,
    ) -> Result<String, LarkDiagnostics> {
        match &source.locator {
            LarkSheetLocator::SpreadsheetToken(token) => Ok(token.trim().to_string()),
            LarkSheetLocator::Url(url) => self.spreadsheet_token_from_url(url, auth),
        }
    }

    fn spreadsheet_token_from_url(
        &self,
        url: &str,
        auth: &LarkAuth,
    ) -> Result<String, LarkDiagnostics> {
        if let Some(token) = token_after_path_marker(url, "/sheets/") {
            return Ok(token);
        }
        let cache_key = (auth.key.clone(), url.to_string());
        if let Ok(state) = self.state.lock() {
            if let Some(token) = state.spreadsheet_tokens.get(&cache_key) {
                return Ok(token.clone());
            }
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
        let response = self.request(
            auth,
            &LarkRequest {
                method: LarkHttpMethod::Get,
                code: "LARK-WIKI",
                description: "wiki node",
                endpoint: &endpoint,
                body: None,
            },
        )?;
        let envelope: ApiEnvelope<WikiNodeData> =
            parse_response("LARK-WIKI", "wiki node", &response)?;
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
        let token = data.node.obj_token;
        if let Ok(mut state) = self.state.lock() {
            state.spreadsheet_tokens.insert(cache_key, token.clone());
        }
        Ok(token)
    }

    pub(crate) fn metadata(
        &self,
        spreadsheet_token: &str,
        auth: &LarkAuth,
    ) -> Result<Vec<LarkSheetMetadata>, LarkDiagnostics> {
        let cache_key = (auth.key.clone(), spreadsheet_token.to_string());
        if let Ok(state) = self.state.lock() {
            if let Some(sheets) = state.sheets.get(&cache_key) {
                return Ok(sheets.ordered.clone());
            }
        }
        let endpoint = format!(
            "{API_BASE}/sheets/v3/spreadsheets/{}/sheets/query",
            url_component(spreadsheet_token)
        );
        let response = self.request(
            auth,
            &LarkRequest {
                method: LarkHttpMethod::Get,
                code: "LARK-SHEET",
                description: "spreadsheet sheets",
                endpoint: &endpoint,
                body: None,
            },
        )?;
        let envelope: ApiEnvelope<SheetsQueryData> =
            parse_response("LARK-SHEET", "spreadsheet sheets", &response)?;
        let metadata = envelope_data(envelope, "LARK-SHEET", "spreadsheet sheets")?.sheets;
        if let Ok(mut state) = self.state.lock() {
            state
                .sheets
                .insert(cache_key, CachedSheets::new(metadata.clone()));
        }
        Ok(metadata)
    }

    pub(crate) fn sheet_metadata(
        &self,
        spreadsheet_token: &str,
        sheet: &str,
        auth: &LarkAuth,
    ) -> Result<LarkSheetMetadata, LarkDiagnostics> {
        let cache_key = (auth.key.clone(), spreadsheet_token.to_string());
        if let Ok(state) = self.state.lock() {
            if let Some(metadata) = state
                .sheets
                .get(&cache_key)
                .and_then(|sheets| sheets.by_name_or_id.get(sheet))
            {
                return Ok(metadata.clone());
            }
        }
        let metadata = self.metadata(spreadsheet_token, auth)?;
        metadata
            .into_iter()
            .find(|metadata| metadata.title == sheet || metadata.sheet_id == sheet)
            .ok_or_else(|| {
                LarkDiagnostics::one(LarkDiagnostic::new(
                    "LARK-SHEET",
                    format!("sheet `{sheet}` not found in spreadsheet"),
                ))
            })
    }
}

fn remote_error(
    code: &str,
    method: &str,
    description: &str,
    message: impl std::fmt::Display,
) -> LarkDiagnostics {
    LarkDiagnostics::one(LarkDiagnostic::new(
        code,
        format!("{method} {description} failed: {message}"),
    ))
}

#[derive(Deserialize)]
struct ResponseCode {
    code: i64,
}

fn token_expired(response: &str) -> bool {
    serde_json::from_str::<ResponseCode>(response)
        .is_ok_and(|response| (99_991_000..100_000_000).contains(&response.code))
}

pub(crate) fn parse_response<T: DeserializeOwned>(
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

pub(crate) fn envelope_data<T>(
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

impl Default for LarkSheetLoader<UreqLarkHttpClient> {
    fn default() -> Self {
        Self::new(UreqLarkHttpClient)
    }
}

impl Default for LarkSheetWriter<UreqLarkHttpClient> {
    fn default() -> Self {
        Self::new(UreqLarkHttpClient)
    }
}

impl<C> LarkSheetLoader<C> {
    #[must_use]
    pub fn new(client: C) -> Self {
        Self::from_remote(Arc::new(LarkRemote::new(client)))
    }

    pub(crate) fn from_remote(remote: Arc<LarkRemote<C>>) -> Self {
        Self { remote }
    }
}

impl<C> LarkSheetWriter<C> {
    #[must_use]
    pub fn new(client: C) -> Self {
        Self::from_remote(Arc::new(LarkRemote::new(client)))
    }

    pub(crate) fn from_remote(remote: Arc<LarkRemote<C>>) -> Self {
        Self { remote }
    }
}

pub fn lark_provider_roles<C>(client: C) -> (LarkSheetLoader<C>, LarkSheetWriter<C>) {
    let remote = Arc::new(LarkRemote::new(client));
    (
        LarkSheetLoader::from_remote(Arc::clone(&remote)),
        LarkSheetWriter::from_remote(remote),
    )
}

impl<C> LarkSheetWriter<C>
where
    C: LarkHttpClient,
{
    pub(crate) fn lark_write_auth(
        &self,
        source: &ResolvedSource,
    ) -> Result<LarkAuth, DiagnosticSet> {
        let options = crate::source::lark_source_options(source)?;
        self.remote
            .authenticate(&options.app_id, &options.app_secret)
            .map_err(lark_diagnostics_to_api)
    }

    pub(crate) fn lark_spreadsheet_token_from_source(
        &self,
        source: &ResolvedSource,
        auth: &LarkAuth,
    ) -> Result<String, DiagnosticSet> {
        let source = lark_source_from_spec(source)?;
        self.remote
            .spreadsheet_token(&source, auth)
            .map_err(lark_diagnostics_to_api)
    }

    pub(crate) fn cached_sheet_metadata(
        &self,
        spreadsheet_token: &str,
        sheet: &str,
        auth: &LarkAuth,
    ) -> Result<LarkSheetMetadata, DiagnosticSet> {
        self.remote
            .sheet_metadata(spreadsheet_token, sheet, auth)
            .map_err(lark_diagnostics_to_api)
    }

    pub(crate) fn cached_sheet_id(
        &self,
        spreadsheet_token: &str,
        sheet: &str,
        auth: &LarkAuth,
    ) -> Result<String, DiagnosticSet> {
        self.cached_sheet_metadata(spreadsheet_token, sheet, auth)
            .map(|metadata| metadata.sheet_id)
    }

    pub(crate) fn invalidate_document(&self, auth: &LarkAuth, spreadsheet_token: &str) {
        self.remote.invalidate_document(auth, spreadsheet_token);
    }

    pub(crate) fn send_remote_request(
        &self,
        auth: &LarkAuth,
        request: &LarkRequest<'_>,
    ) -> Result<String, DiagnosticSet> {
        self.remote
            .request(auth, request)
            .map_err(lark_diagnostics_to_api)
    }
}
