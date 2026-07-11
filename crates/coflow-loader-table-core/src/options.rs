use crate::TableSheetConfig;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableSourceOptions {
    source_label: &'static str,
    sheets: Vec<TableSheetConfig>,
    sheets_by_name: BTreeMap<String, usize>,
    sheets_by_type: BTreeMap<String, Vec<usize>>,
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
        let sheets = table_sheet_configs_from_options(options, source_label)?;
        let mut sheets_by_name = BTreeMap::new();
        let mut sheets_by_type = BTreeMap::<String, Vec<usize>>::new();
        for (index, sheet) in sheets.iter().enumerate() {
            if sheets_by_name.insert(sheet.sheet.clone(), index).is_some() {
                return Err(TableOptionsError::new(format!(
                    "{source_label} defines duplicate sheet `{}`",
                    sheet.sheet
                )));
            }
            if let Some(type_name) = &sheet.type_name {
                sheets_by_type
                    .entry(type_name.clone())
                    .or_default()
                    .push(index);
            }
        }
        Ok(Self {
            source_label,
            sheets,
            sheets_by_name,
            sheets_by_type,
        })
    }

    #[must_use]
    pub fn empty() -> Self {
        Self {
            source_label: "table source",
            sheets: Vec::new(),
            sheets_by_name: BTreeMap::new(),
            sheets_by_type: BTreeMap::new(),
        }
    }

    #[must_use]
    pub fn sheets(&self) -> &[TableSheetConfig] {
        &self.sheets
    }

    #[must_use]
    pub fn into_sheets(self) -> Vec<TableSheetConfig> {
        self.sheets
    }

    /// Resolve options for an explicitly addressed sheet.
    ///
    /// # Errors
    ///
    /// Returns an error when the sheet is configured for a different type.
    pub fn sheet_config(
        &self,
        sheet: &str,
        actual_type: &str,
    ) -> Result<TableSheetConfig, TableOptionsError> {
        let Some(index) = self.sheets_by_name.get(sheet) else {
            return Ok(TableSheetConfig::new(sheet).with_type(actual_type));
        };
        let config = &self.sheets[*index];
        if let Some(configured_type) = &config.type_name {
            if configured_type != actual_type {
                return Err(TableOptionsError::new(format!(
                    "{} sheet `{sheet}` is configured for type `{configured_type}`, not `{actual_type}`",
                    self.source_label
                )));
            }
        }
        Ok(config.clone())
    }

    /// Resolve the only configured sheet for a type.
    ///
    /// # Errors
    ///
    /// Returns an error when the type is mapped to multiple sheets and an
    /// explicit sheet is required to select one.
    pub fn sheet_for_type(&self, actual_type: &str) -> Result<Option<&str>, TableOptionsError> {
        let Some(indexes) = self.sheets_by_type.get(actual_type) else {
            return Ok(None);
        };
        if let [index] = indexes.as_slice() {
            return Ok(Some(self.sheets[*index].sheet.as_str()));
        }
        let names = indexes
            .iter()
            .map(|index| self.sheets[*index].sheet.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        Err(TableOptionsError::new(format!(
            "{} type `{actual_type}` is configured for multiple sheets ({names}); specify a sheet",
            self.source_label
        )))
    }

    /// Resolve the configured type for an explicitly addressed sheet.
    ///
    /// # Errors
    ///
    /// Returns an error when no sheet was specified and the source contains
    /// multiple candidate sheets.
    pub fn type_for_sheet(&self, sheet: Option<&str>) -> Result<Option<&str>, TableOptionsError> {
        if let Some(sheet) = sheet {
            return Ok(self
                .sheets_by_name
                .get(sheet)
                .and_then(|index| self.sheets[*index].type_name.as_deref()));
        }
        match self.sheets.as_slice() {
            [] => Ok(None),
            [config] => Ok(config.type_name.as_deref()),
            _ => Err(TableOptionsError::new(format!(
                "{} defines multiple sheets; specify a sheet",
                self.source_label
            ))),
        }
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
        let mut target_fields = BTreeSet::new();
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
            if !target_fields.insert(field) {
                return Err(TableOptionsError::new(format!(
                    "{source_label} sheet `{sheet_name}` maps multiple columns to field `{field}`"
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
