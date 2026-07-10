use coflow_api::{Diagnostic, DiagnosticSet, ResolvedSource, SourceLocationSpec};
use coflow_loader_table_core::{TableSheetConfig, TableSourceOptions};
use serde_json::Value;

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
        lark_table_options_from_options(options)?.into_sheets(),
    ))
}

pub(crate) fn is_lark_uri(uri: &str) -> bool {
    lark_token_uri(uri).is_some()
        || (uri.starts_with("https://") && (uri.contains("feishu") || uri.contains("larksuite")))
}

pub(crate) fn required_option_string(options: &Value, key: &str) -> Result<String, DiagnosticSet> {
    option_string(options, key).ok_or_else(|| {
        DiagnosticSet::one(Diagnostic::error(
            "LARK-SOURCE",
            "LARK",
            format!("lark source requires `{key}`"),
        ))
    })
}

fn lark_token_uri(uri: &str) -> Option<&str> {
    let token = uri.strip_prefix("lark:")?;
    (!token.trim().is_empty()).then_some(token)
}

fn option_string(options: &Value, key: &str) -> Option<String> {
    options.get(key).and_then(Value::as_str).map(str::to_string)
}

pub(crate) fn sheet_config_from_options(
    options: &Value,
    sheet: &str,
    actual_type: &str,
) -> Result<TableSheetConfig, DiagnosticSet> {
    Ok(lark_table_options_from_options(options)?.sheet_config(sheet, actual_type))
}

pub(crate) fn sheet_for_type_from_options(
    options: &Value,
    actual_type: &str,
) -> Result<Option<String>, DiagnosticSet> {
    Ok(lark_table_options_from_options(options)?
        .sheet_for_type(actual_type)
        .map(ToOwned::to_owned))
}

pub(crate) fn lark_document(source: &LarkSheetSource) -> String {
    match &source.locator {
        LarkSheetLocator::Url(url) => url.clone(),
        LarkSheetLocator::SpreadsheetToken(token) => format!("lark:{token}"),
    }
}

fn lark_table_options_from_options(options: &Value) -> Result<TableSourceOptions, DiagnosticSet> {
    TableSourceOptions::decode(options, "lark source").map_err(|err| {
        DiagnosticSet::one(Diagnostic::error("LARK-SOURCE", "LARK", err.message))
    })
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
