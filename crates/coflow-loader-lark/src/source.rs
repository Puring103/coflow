use coflow_api::{Diagnostic, DiagnosticSet, ResolvedSource, SourceLocationSpec};
use coflow_loader_table_core::TableSheetConfig;
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
        table_sheet_configs_from_options(options)?,
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

pub(crate) fn sheet_config_from_options(
    options: &Value,
    sheet: &str,
    actual_type: &str,
) -> Result<TableSheetConfig, DiagnosticSet> {
    for config in table_sheet_configs_from_options(options)? {
        let matches_sheet = config.sheet == sheet;
        let matches_type = config
            .type_name
            .as_deref()
            .is_some_and(|candidate| candidate == actual_type);
        if matches_sheet || matches_type {
            return Ok(config);
        }
    }
    Ok(TableSheetConfig::new(sheet).with_type(actual_type))
}

pub(crate) fn sheet_for_type_from_options<'a>(
    options: &'a Value,
    actual_type: &str,
) -> Option<&'a str> {
    options
        .get("sheets")
        .and_then(Value::as_array)?
        .iter()
        .filter_map(Value::as_object)
        .find(|object| {
            object
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(|candidate| candidate == actual_type)
        })?
        .get("sheet")
        .and_then(Value::as_str)
}

pub(crate) fn lark_document(source: &LarkSheetSource) -> String {
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
