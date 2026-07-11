use crate::LARK_SHEET_LOADER_DESCRIPTOR;
use coflow_api::{
    DecodedSourceOptions, Diagnostic, DiagnosticSet, Label, ResolvedSource, SourceLocation,
    SourceLocationSpec,
};
use coflow_loader_table_core::{TableSheetConfig, TableSourceOptions};
use serde_json::Value;
use std::fmt;

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct LarkSourceOptions {
    pub(crate) app_id: String,
    pub(crate) app_secret: String,
    url: Option<String>,
    spreadsheet_token: Option<String>,
    table: TableSourceOptions,
}

impl fmt::Debug for LarkSourceOptions {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LarkSourceOptions")
            .field("app_id", &self.app_id)
            .field("app_secret", &"[redacted]")
            .field("url", &self.url)
            .field("spreadsheet_token", &self.spreadsheet_token)
            .field("table", &self.table)
            .finish()
    }
}

pub(crate) fn decode_lark_source_options(
    raw: &Value,
) -> Result<DecodedSourceOptions, DiagnosticSet> {
    let Some(options) = raw.as_object() else {
        return Err(option_error([], "lark source options must be an object"));
    };
    for key in options.keys() {
        if !["app_id", "app_secret", "url", "spreadsheet_token", "sheets"].contains(&key.as_str()) {
            return Err(option_error(
                [key.as_str()],
                format!("unknown lark source option `{key}`"),
            ));
        }
    }
    let app_id = required_string(options, "app_id")?;
    let app_secret = required_string(options, "app_secret")?;
    let url = optional_string(options, "url")?;
    let spreadsheet_token = optional_string(options, "spreadsheet_token")?;
    let table = TableSourceOptions::decode(raw, "lark source").map_err(lark_options_diagnostics)?;
    Ok(DecodedSourceOptions::new(
        LARK_SHEET_LOADER_DESCRIPTOR.id,
        LarkSourceOptions {
            app_id,
            app_secret,
            url,
            spreadsheet_token,
            table,
        },
    ))
}

pub(crate) fn lark_source_options(
    source: &ResolvedSource,
) -> Result<&LarkSourceOptions, DiagnosticSet> {
    source.options(LARK_SHEET_LOADER_DESCRIPTOR.id)
}

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

pub(crate) fn lark_source_from_spec(
    source: &ResolvedSource,
) -> Result<LarkSheetSource, DiagnosticSet> {
    let options = lark_source_options(source)?;
    let source_url = match &source.location {
        SourceLocationSpec::Uri(uri) => Some(uri.clone()),
        SourceLocationSpec::Path(_) => None,
    };
    let url = options.url.clone().or_else(|| source_url.clone());
    let spreadsheet_token = options.spreadsheet_token.clone().or_else(|| {
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
            return Err(error(
                "lark source must set exactly one of `url` or `spreadsheet_token`",
            ))
        }
        (None, None) => return Err(error("lark source requires `url` or `spreadsheet_token`")),
    };
    Ok(LarkSheetSource::new(
        options.app_id.clone(),
        options.app_secret.clone(),
        locator,
        options.table.clone().into_sheets(),
    ))
}

pub(crate) fn is_lark_uri(uri: &str) -> bool {
    lark_token_uri(uri).is_some()
        || (uri.starts_with("https://") && (uri.contains("feishu") || uri.contains("larksuite")))
}

fn lark_token_uri(uri: &str) -> Option<&str> {
    let token = uri.strip_prefix("lark:")?;
    (!token.trim().is_empty()).then_some(token)
}

pub(crate) fn sheet_config_from_options(
    options: &LarkSourceOptions,
    sheet: &str,
    actual_type: &str,
) -> Result<TableSheetConfig, DiagnosticSet> {
    options
        .table
        .sheet_config(sheet, actual_type)
        .map_err(lark_options_diagnostics)
}

pub(crate) fn sheet_for_type_from_options(
    options: &LarkSourceOptions,
    actual_type: &str,
) -> Result<Option<String>, DiagnosticSet> {
    Ok(options
        .table
        .sheet_for_type(actual_type)
        .map_err(lark_options_diagnostics)?
        .map(ToOwned::to_owned))
}

pub(crate) fn type_for_sheet_from_options(
    options: &LarkSourceOptions,
    sheet: Option<&str>,
) -> Result<Option<String>, DiagnosticSet> {
    Ok(options
        .table
        .type_for_sheet(sheet)
        .map_err(lark_options_diagnostics)?
        .map(ToOwned::to_owned))
}

pub(crate) fn lark_document(source: &LarkSheetSource) -> String {
    match &source.locator {
        LarkSheetLocator::Url(url) => url.clone(),
        LarkSheetLocator::SpreadsheetToken(token) => format!("lark:{token}"),
    }
}

fn required_string(
    options: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<String, DiagnosticSet> {
    let value = optional_string(options, key)?
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| option_error([key], format!("lark source requires `{key}`")))?;
    Ok(value)
}

fn optional_string(
    options: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<Option<String>, DiagnosticSet> {
    let Some(value) = options.get(key) else {
        return Ok(None);
    };
    value
        .as_str()
        .map(|value| Some(value.to_string()))
        .ok_or_else(|| {
            option_error(
                [key],
                format!("lark source option `{key}` must be a string"),
            )
        })
}

fn lark_options_diagnostics(err: coflow_loader_table_core::TableOptionsError) -> DiagnosticSet {
    option_error(["sheets"], err.message)
}

fn error(message: impl Into<String>) -> DiagnosticSet {
    DiagnosticSet::one(Diagnostic::error("LARK-SOURCE", "LARK", message))
}

fn option_error<'a>(
    key_path: impl IntoIterator<Item = &'a str>,
    message: impl Into<String>,
) -> DiagnosticSet {
    DiagnosticSet::one(
        Diagnostic::error("LARK-SOURCE", "LARK", message).with_primary(Label {
            location: SourceLocation::ProjectConfig {
                path: std::path::PathBuf::new(),
                key_path: key_path.into_iter().map(str::to_string).collect(),
            },
            message: None,
        }),
    )
}

pub(crate) fn token_after_path_marker(url: &str, marker: &str) -> Option<String> {
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

pub(crate) fn lark_document_spreadsheet_token(document: &str) -> Option<String> {
    document
        .strip_prefix("lark:")
        .map(str::to_string)
        .or_else(|| token_after_path_marker(document, "/sheets/"))
}
