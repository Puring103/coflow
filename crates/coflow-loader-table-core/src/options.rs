use crate::TableSheetConfig;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableSourceOptions {
    sheets: Vec<TableSheetConfig>,
}

impl TableSourceOptions {
    /// Decode the common table-source `sheets` option shape used by CSV,
    /// Excel, and remote table providers.
    ///
    /// `source_label` is included in diagnostics, e.g. `csv source`.
    ///
    /// # Errors
    ///
    /// Returns an error when `sheets` or any nested sheet field has the wrong
    /// shape.
    pub fn decode(options: &Value, source_label: &'static str) -> Result<Self, TableOptionsError> {
        Ok(Self {
            sheets: table_sheet_configs_from_options(options, source_label)?,
        })
    }

    #[must_use]
    pub fn empty() -> Self {
        Self { sheets: Vec::new() }
    }

    #[must_use]
    pub fn sheets(&self) -> &[TableSheetConfig] {
        &self.sheets
    }

    #[must_use]
    pub fn into_sheets(self) -> Vec<TableSheetConfig> {
        self.sheets
    }

    #[must_use]
    pub fn matching_sheet(&self, sheet: &str, actual_type: &str) -> Option<&TableSheetConfig> {
        self.sheets.iter().find(|config| {
            config.sheet == sheet
                || config
                    .type_name
                    .as_deref()
                    .is_some_and(|candidate| candidate == actual_type)
        })
    }

    #[must_use]
    pub fn sheet_config(&self, sheet: &str, actual_type: &str) -> TableSheetConfig {
        self.matching_sheet(sheet, actual_type)
            .cloned()
            .unwrap_or_else(|| TableSheetConfig::new(sheet).with_type(actual_type))
    }

    #[must_use]
    pub fn sheet_for_type(&self, actual_type: &str) -> Option<&str> {
        self.sheets
            .iter()
            .find(|config| {
                config
                    .type_name
                    .as_deref()
                    .is_some_and(|candidate| candidate == actual_type)
            })
            .map(|config| config.sheet.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableOptionsError {
    pub message: String,
}

impl TableOptionsError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

fn table_sheet_configs_from_options(
    options: &Value,
    source_label: &'static str,
) -> Result<Vec<TableSheetConfig>, TableOptionsError> {
    let Some(sheets) = options.get("sheets") else {
        return Ok(Vec::new());
    };
    let Some(sheets) = sheets.as_array() else {
        return Err(TableOptionsError::new(format!(
            "{source_label} option `sheets` must be an array"
        )));
    };
    sheets
        .iter()
        .map(|value| table_sheet_config_from_value(value, source_label))
        .collect::<Result<Vec<_>, _>>()
}

fn table_sheet_config_from_value(
    value: &Value,
    source_label: &'static str,
) -> Result<TableSheetConfig, TableOptionsError> {
    let Some(object) = value.as_object() else {
        return Err(TableOptionsError::new(format!(
            "{source_label} sheet config must be an object"
        )));
    };
    let Some(sheet_name) = object.get("sheet").and_then(Value::as_str) else {
        return Err(TableOptionsError::new(format!(
            "{source_label} sheet config requires `sheet`"
        )));
    };
    if sheet_name.trim().is_empty() {
        return Err(TableOptionsError::new(format!(
            "{source_label} sheet `sheet` is empty"
        )));
    }
    let mut sheet = TableSheetConfig::new(sheet_name);
    if let Some(type_name) =
        optional_string_field(object, "type", &format!("{source_label} sheet `type`"))?
    {
        if type_name.trim().is_empty() {
            return Err(TableOptionsError::new(format!(
                "{source_label} sheet `type` is empty"
            )));
        }
        sheet = sheet.with_type(type_name);
    }
    if let Some(key) = optional_string_field(object, "key", &format!("{source_label} sheet `key`"))?
    {
        if key.trim().is_empty() {
            return Err(TableOptionsError::new(format!(
                "{source_label} sheet `key` is empty"
            )));
        }
        sheet = sheet.with_key(key);
    }
    if let Some(columns) = object.get("columns") {
        let Some(columns) = columns.as_object() else {
            return Err(TableOptionsError::new(format!(
                "{source_label} sheet `columns` must be an object"
            )));
        };
        let mut parsed_columns = Vec::new();
        for (source, field) in columns {
            let Some(field) = field.as_str() else {
                return Err(TableOptionsError::new(format!(
                    "{source_label} sheet column `{source}` must map to a string field"
                )));
            };
            if source.trim().is_empty() {
                return Err(TableOptionsError::new(format!(
                    "{source_label} sheet column name is empty"
                )));
            }
            if field.trim().is_empty() {
                return Err(TableOptionsError::new(format!(
                    "{source_label} sheet column `{source}` maps to an empty field"
                )));
            }
            parsed_columns.push((source.as_str(), field));
        }
        sheet = sheet.with_columns(parsed_columns);
    }
    Ok(sheet)
}

fn optional_string_field<'a>(
    object: &'a serde_json::Map<String, Value>,
    key: &str,
    label: &str,
) -> Result<Option<&'a str>, TableOptionsError> {
    let Some(value) = object.get(key) else {
        return Ok(None);
    };
    value
        .as_str()
        .map(Some)
        .ok_or_else(|| TableOptionsError::new(format!("{label} must be a string")))
}
