use coflow_api::{
    DecodedSourceOptions, Diagnostic, DiagnosticSet, LoadedSource, ProbeResult, ProjectSourceRef,
    ResolvedSource, SourceLoadContext, SourceLocationSpec, SourceProvider,
    SourceProviderDescriptor, SourceResolveContext,
};
use coflow_loader_table_core::{
    collect_table_input_records, TableSheet, TableSheetConfig, TableSource,
};
use serde_json::Value;

use crate::diagnostics::{lark_diagnostics_to_api, table_diagnostics_to_api};
use crate::dto::{ApiEnvelope, LarkSheetMetadata, ValuesData};
use crate::http::{LarkHttpClient, UreqLarkHttpClient};
use crate::remote::{
    envelope_data, parse_response, LarkAuth, LarkHttpMethod, LarkRemote, LarkRequest,
};
use crate::source::{
    decode_lark_source_options, is_lark_uri, lark_document, lark_source_from_spec, LarkSheetSource,
};
use crate::{
    column_name, json_cell_text, url_component, LarkDiagnostic, LarkDiagnostics, API_BASE,
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
    let remote = LarkRemote::new(client);
    load_lark_table_source_with_remote(source, &remote)
}

fn load_lark_table_source_with_remote<C>(
    source: &LarkSheetSource,
    remote: &LarkRemote<C>,
) -> Result<TableSource, LarkDiagnostics>
where
    C: LarkHttpClient,
{
    let auth = remote.authenticate(&source.app_id, &source.app_secret)?;
    let spreadsheet_token = remote.spreadsheet_token(source, &auth)?;
    let metadata = remote.metadata(&spreadsheet_token, &auth)?;
    build_table_source(remote, source, &spreadsheet_token, &auth, &metadata)
}

fn build_table_source<C>(
    remote: &LarkRemote<C>,
    source: &LarkSheetSource,
    spreadsheet_token: &str,
    auth: &LarkAuth,
    metadata: &[LarkSheetMetadata],
) -> Result<TableSource, LarkDiagnostics>
where
    C: LarkHttpClient,
{
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
            sheet_values(remote, spreadsheet_token, sheet, auth)?
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

fn sheet_values<C>(
    remote: &LarkRemote<C>,
    spreadsheet_token: &str,
    sheet: &LarkSheetMetadata,
    auth: &LarkAuth,
) -> Result<Vec<Vec<String>>, LarkDiagnostics>
where
    C: LarkHttpClient,
{
    let last_column = column_name(sheet.column_count());
    let range = format!("{}!A1:{last_column}{}", sheet.sheet_id, sheet.row_count());
    let endpoint = format!(
        "{API_BASE}/sheets/v2/spreadsheets/{}/values/{}?valueRenderOption=ToString",
        url_component(spreadsheet_token),
        url_component(&range)
    );
    let response = remote.request(
        auth,
        &LarkRequest {
            method: LarkHttpMethod::Get,
            code: "LARK-VALUE",
            description: "spreadsheet values",
            endpoint: &endpoint,
            body: None,
        },
    )?;
    let envelope: ApiEnvelope<ValuesData> =
        parse_response("LARK-VALUE", "spreadsheet values", &response)?;
    let data = envelope_data(envelope, "LARK-VALUE", "spreadsheet values")?;
    Ok(data.value_range.values.into_iter().map(json_row).collect())
}

fn json_row(row: Vec<Value>) -> Vec<String> {
    row.into_iter().map(json_cell_text).collect()
}

#[derive(Debug, Clone)]
pub struct LarkSheetLoader<C = UreqLarkHttpClient> {
    pub(crate) remote: std::sync::Arc<LarkRemote<C>>,
}

impl<C> LarkSheetLoader<C>
where
    C: LarkHttpClient + Send + Sync,
{
    fn load_lark_table_source_cached(
        &self,
        source: &LarkSheetSource,
    ) -> Result<TableSource, LarkDiagnostics> {
        load_lark_table_source_with_remote(source, &self.remote)
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
