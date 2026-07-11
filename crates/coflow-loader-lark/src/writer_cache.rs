use std::collections::HashMap;

use coflow_api::{DiagnosticSet, ResolvedSource};
use serde_json::json;

use crate::diagnostics::diag;
use crate::dto::{ApiEnvelope, AuthResponse, LarkSheetMetadata, SheetsQueryData};
use crate::http::{LarkHttpClient, UreqLarkHttpClient};
use crate::load::spreadsheet_token;
use crate::source::lark_source_from_spec;
use crate::{api_error_message, url_component, LarkSheetWriter, API_BASE, AUTH_URL};

#[derive(Debug, Default)]
pub(crate) struct LarkWriterCache {
    /// Keyed by `app_id` — values represent a tenant access token + the
    /// instant after which it is considered stale.
    tokens: HashMap<String, CachedToken>,
    /// Keyed by `spreadsheet_token` — values are sheet-title/id → metadata
    /// captured the first time we hit the spreadsheet.
    sheets: HashMap<String, HashMap<String, LarkSheetMetadata>>,
}

#[derive(Debug, Clone)]
struct CachedToken {
    token: String,
    /// `Instant` after which the cached token must be refreshed.
    expires_at: std::time::Instant,
}

#[derive(Debug, Clone)]
pub(crate) struct LarkWriteAuth {
    pub(crate) app_id: String,
    pub(crate) app_secret: String,
    pub(crate) token: String,
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
    pub(crate) fn cached_tenant_token(
        &self,
        app_id: &str,
        app_secret: &str,
    ) -> Result<String, DiagnosticSet> {
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

    /// Look up sheet metadata for a sheet title or id in a given spreadsheet,
    /// fetching the spreadsheet's metadata once and caching the full
    /// title/id map for subsequent lookups.
    pub(crate) fn cached_sheet_metadata(
        &self,
        spreadsheet_token: &str,
        sheet_title: &str,
        tenant_token: &str,
    ) -> Result<LarkSheetMetadata, DiagnosticSet> {
        if let Ok(cache) = self.cache.lock() {
            if let Some(map) = cache.sheets.get(spreadsheet_token) {
                if let Some(metadata) = map.get(sheet_title) {
                    return Ok(metadata.clone());
                }
                // The same spreadsheet might already be cached, but the
                // particular title has not been resolved yet. Fall through
                // to fetch + insert without invalidating siblings.
            }
        }
        let map = fetch_sheet_metadata_map(&self.client, spreadsheet_token, tenant_token)?;
        let resolved = map.get(sheet_title).cloned().ok_or_else(|| {
            DiagnosticSet::one(diag(
                "LARK-WRITE",
                format!("sheet `{sheet_title}` not found in spreadsheet"),
            ))
        })?;
        if let Ok(mut cache) = self.cache.lock() {
            cache.sheets.insert(spreadsheet_token.to_string(), map);
        }
        Ok(resolved)
    }

    pub(crate) fn cached_sheet_id(
        &self,
        spreadsheet_token: &str,
        sheet_title: &str,
        tenant_token: &str,
    ) -> Result<String, DiagnosticSet> {
        self.cached_sheet_metadata(spreadsheet_token, sheet_title, tenant_token)
            .map(|metadata| metadata.sheet_id)
    }

    /// Drop cached entries for an `app_id` / spreadsheet pair after a write
    /// fails with auth or sheet-not-found errors. Called by the writer's
    /// retry path.
    pub(crate) fn invalidate_caches(&self, app_id: Option<&str>, spreadsheet_token: Option<&str>) {
        if let Ok(mut cache) = self.cache.lock() {
            if let Some(app) = app_id {
                cache.tokens.remove(app);
            }
            if let Some(token) = spreadsheet_token {
                cache.sheets.remove(token);
            }
        }
    }

    pub(crate) fn lark_spreadsheet_token_from_source(
        &self,
        source: &ResolvedSource,
        tenant_access_token: &str,
    ) -> Result<String, DiagnosticSet> {
        let lark_source = lark_source_from_spec(source)?;
        match spreadsheet_token(&self.client, &lark_source, tenant_access_token) {
            Ok(token) => Ok(token),
            Err(err) => Err(crate::diagnostics::lark_diagnostics_to_api(err)),
        }
    }

    pub(crate) fn lark_write_auth(
        &self,
        source: &ResolvedSource,
    ) -> Result<LarkWriteAuth, DiagnosticSet> {
        let options = crate::source::lark_source_options(source)?;
        let app_id = options.app_id.clone();
        let app_secret = options.app_secret.clone();
        let token = self.cached_tenant_token(&app_id, &app_secret)?;
        Ok(LarkWriteAuth {
            app_id,
            app_secret,
            token,
        })
    }
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

/// Fetch sheet metadata keyed by both title and id so either form can be used
/// without discarding row/column bounds needed by table operations.
pub(crate) fn fetch_sheet_metadata_map(
    client: &impl LarkHttpClient,
    spreadsheet_token: &str,
    tenant_token: &str,
) -> Result<HashMap<String, LarkSheetMetadata>, DiagnosticSet> {
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
    let mut map = HashMap::new();
    for sheet in data.sheets {
        map.insert(sheet.title.clone(), sheet.clone());
        map.insert(sheet.sheet_id.clone(), sheet);
    }
    Ok(map)
}
