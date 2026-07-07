use crate::ExcelSheet;
use coflow_api::{Diagnostic, DiagnosticSet};
use serde_json::Value;

pub(super) fn excel_sheets_from_options(options: &Value) -> Result<Vec<ExcelSheet>, DiagnosticSet> {
    let Some(sheets) = options.get("sheets") else {
        return Ok(Vec::new());
    };
    let Some(sheets) = sheets.as_array() else {
        return Err(DiagnosticSet::one(Diagnostic::error(
            "EXCEL-SOURCE",
            "EXCEL",
            "excel source option `sheets` must be an array",
        )));
    };
    sheets
        .iter()
        .map(excel_sheet_from_value)
        .collect::<Result<Vec<_>, _>>()
}

fn excel_sheet_from_value(value: &Value) -> Result<ExcelSheet, DiagnosticSet> {
    let Some(object) = value.as_object() else {
        return Err(DiagnosticSet::one(Diagnostic::error(
            "EXCEL-SOURCE",
            "EXCEL",
            "excel source sheet config must be an object",
        )));
    };
    let Some(sheet_name) = object.get("sheet").and_then(Value::as_str) else {
        return Err(DiagnosticSet::one(Diagnostic::error(
            "EXCEL-SOURCE",
            "EXCEL",
            "excel source sheet config requires `sheet`",
        )));
    };
    if sheet_name.trim().is_empty() {
        return Err(DiagnosticSet::one(Diagnostic::error(
            "EXCEL-SOURCE",
            "EXCEL",
            "excel source sheet `sheet` is empty",
        )));
    }
    let mut sheet = ExcelSheet::new(sheet_name);
    if let Some(type_name) = optional_string_field(object, "type", "excel source sheet `type`")? {
        if type_name.trim().is_empty() {
            return Err(DiagnosticSet::one(Diagnostic::error(
                "EXCEL-SOURCE",
                "EXCEL",
                "excel source sheet `type` is empty",
            )));
        }
        sheet = sheet.with_type(type_name);
    }
    if let Some(key) = optional_string_field(object, "key", "excel source sheet `key`")? {
        if key.trim().is_empty() {
            return Err(DiagnosticSet::one(Diagnostic::error(
                "EXCEL-SOURCE",
                "EXCEL",
                "excel source sheet `key` is empty",
            )));
        }
        sheet = sheet.with_key(key);
    }
    if let Some(columns) = object.get("columns") {
        let Some(columns) = columns.as_object() else {
            return Err(DiagnosticSet::one(Diagnostic::error(
                "EXCEL-SOURCE",
                "EXCEL",
                "excel source sheet `columns` must be an object",
            )));
        };
        let mut parsed_columns = Vec::new();
        for (source, field) in columns {
            let Some(field) = field.as_str() else {
                return Err(DiagnosticSet::one(Diagnostic::error(
                    "EXCEL-SOURCE",
                    "EXCEL",
                    format!("excel source sheet column `{source}` must map to a string field"),
                )));
            };
            if source.trim().is_empty() {
                return Err(DiagnosticSet::one(Diagnostic::error(
                    "EXCEL-SOURCE",
                    "EXCEL",
                    "excel source sheet column name is empty",
                )));
            }
            if field.trim().is_empty() {
                return Err(DiagnosticSet::one(Diagnostic::error(
                    "EXCEL-SOURCE",
                    "EXCEL",
                    format!("excel source sheet column `{source}` maps to an empty field"),
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
) -> Result<Option<&'a str>, DiagnosticSet> {
    let Some(value) = object.get(key) else {
        return Ok(None);
    };
    value.as_str().map(Some).ok_or_else(|| {
        DiagnosticSet::one(Diagnostic::error(
            "EXCEL-SOURCE",
            "EXCEL",
            format!("{label} must be a string"),
        ))
    })
}
