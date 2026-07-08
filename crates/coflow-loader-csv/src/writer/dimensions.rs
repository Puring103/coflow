use coflow_api::{
    DiagnosticSet, DimensionSourceManager, DimensionSourceManagerDescriptor,
    DimensionSourceOptionsRequest, DimensionSourceRequest, DimensionSourceResult,
    SourceLocationSpec, TableContext,
};
use coflow_loader_table_core::cell_value::{render_cell_value, CellRenderError};
use serde_json::json;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use super::{diag, CsvWriter};
use crate::{parse, write};

pub(super) static CSV_DIMENSION_SOURCE_MANAGER_DESCRIPTOR: DimensionSourceManagerDescriptor =
    DimensionSourceManagerDescriptor {
        id: "csv",
        display_name: "CSV dimension source",
    };

impl DimensionSourceManager for CsvWriter {
    fn descriptor(&self) -> &'static DimensionSourceManagerDescriptor {
        &CSV_DIMENSION_SOURCE_MANAGER_DESCRIPTOR
    }

    fn source_options(&self, request: &DimensionSourceOptionsRequest<'_>) -> serde_json::Value {
        json!({
            "sheets": [{
                "sheet": request.sheet,
                "type": request.actual_type,
            }]
        })
    }

    fn sync_dimension_source(
        &self,
        _ctx: TableContext<'_>,
        request: &DimensionSourceRequest<'_>,
    ) -> Result<DimensionSourceResult, DiagnosticSet> {
        let SourceLocationSpec::Path(path) = &request.source.location else {
            return Err(DiagnosticSet::one(diag(
                "CSV-DIMENSION",
                "csv dimension source requires a local path source",
            )));
        };
        let existing = read_existing_dimension_csv(path, request.variants)?;
        let mut rows = Vec::new();
        let mut header = vec!["id".to_string(), "default".to_string()];
        header.extend(request.variants.iter().cloned());
        rows.push(header);
        for entry in request.entries {
            let mut row = existing.get(&entry.key).cloned().unwrap_or_default();
            row.default = render_dimension_csv_value(&entry.default);
            let mut record = vec![entry.key.clone(), row.default];
            for variant in request.variants {
                record.push(
                    row.variants
                        .get(variant)
                        .map_or_else(|| "null".to_string(), |value| csv_variant_cell(value)),
                );
            }
            rows.push(record);
        }
        let body = write(&rows);
        write_if_changed(path, &body, "CSV-DIMENSION")
    }
}

#[derive(Debug, Clone, Default)]
struct DimensionCsvRow {
    default: String,
    variants: BTreeMap<String, String>,
}

fn read_existing_dimension_csv(
    path: &Path,
    variants: &[String],
) -> Result<BTreeMap<String, DimensionCsvRow>, DiagnosticSet> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(err) => {
            return Err(DiagnosticSet::one(diag(
                "CSV-DIMENSION",
                format!(
                    "failed to read dimension source `{}`: {err}",
                    path.display()
                ),
            )));
        }
    };
    let rows = parse(&text).map_err(|err| {
        DiagnosticSet::one(diag(
            "CSV-DIMENSION",
            format!(
                "failed to parse dimension source `{}`: {err}",
                path.display()
            ),
        ))
    })?;
    let Some(header) = rows.first() else {
        return Ok(BTreeMap::new());
    };
    let Some(id_col) = header.iter().position(|name| name == "id") else {
        return Ok(BTreeMap::new());
    };
    let default_col = header.iter().position(|name| name == "default");
    let variant_cols = variants
        .iter()
        .filter_map(|variant| {
            header
                .iter()
                .position(|name| name == variant)
                .map(|col| (variant.clone(), col))
        })
        .collect::<Vec<_>>();

    let mut out = BTreeMap::new();
    for record in rows.iter().skip(1) {
        let Some(id) = record.get(id_col) else {
            continue;
        };
        let row = out
            .entry(id.clone())
            .or_insert_with(DimensionCsvRow::default);
        if let Some(default_col) = default_col {
            row.default = record.get(default_col).cloned().unwrap_or_default();
        }
        for (variant, col) in &variant_cols {
            if let Some(cell) = record.get(*col) {
                row.variants.insert(variant.clone(), cell.clone());
            }
        }
    }
    Ok(out)
}

fn csv_variant_cell(value: &str) -> String {
    if value.is_empty() {
        "null".to_string()
    } else {
        value.to_string()
    }
}

fn render_dimension_csv_value(value: &coflow_api::CfdValue) -> String {
    match render_cell_value(value) {
        Ok(value) => value,
        Err(CellRenderError::NestedObject | CellRenderError::AnonymousEnum) => {
            render_fallback_cell_value(value)
        }
    }
}

fn render_fallback_cell_value(value: &coflow_api::CfdValue) -> String {
    match value {
        coflow_api::CfdValue::Null => String::new(),
        coflow_api::CfdValue::Bool(value) => value.to_string(),
        coflow_api::CfdValue::Int(value) => value.to_string(),
        coflow_api::CfdValue::Float(value) => value.to_string(),
        coflow_api::CfdValue::String(value) => value.clone(),
        coflow_api::CfdValue::Enum(value) => value.variant.as_deref().map_or_else(
            || format!("{}({})", value.enum_name, value.value),
            |variant| format!("{}.{}", value.enum_name, variant),
        ),
        coflow_api::CfdValue::Object(record) => {
            let inner = record
                .fields()
                .iter()
                .map(|(key, value)| format!("{key}: {}", render_fallback_cell_value(value)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{{{inner}}}")
        }
        coflow_api::CfdValue::Ref(target_key) => format!("&{target_key}"),
        coflow_api::CfdValue::Array(items) => {
            let inner = items
                .iter()
                .map(render_fallback_cell_value)
                .collect::<Vec<_>>()
                .join(", ");
            format!("[{inner}]")
        }
        coflow_api::CfdValue::Dict(entries) => {
            let inner = entries
                .iter()
                .map(|(key, value)| {
                    format!(
                        "{}: {}",
                        render_fallback_dict_key(key),
                        render_fallback_cell_value(value)
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("{{{inner}}}")
        }
    }
}

fn render_fallback_dict_key(key: &coflow_api::CfdDictKey) -> String {
    match key {
        coflow_api::CfdDictKey::String(value) => format!("{value:?}"),
        coflow_api::CfdDictKey::Int(value) => value.to_string(),
        coflow_api::CfdDictKey::Enum(value) => value.variant.as_deref().map_or_else(
            || format!("{}({})", value.enum_name, value.value),
            |variant| format!("{}.{}", value.enum_name, variant),
        ),
    }
}

fn write_if_changed(
    path: &Path,
    body: &str,
    code: &'static str,
) -> Result<DimensionSourceResult, DiagnosticSet> {
    match fs::read_to_string(path) {
        Ok(existing) if existing == body => {
            return Ok(DimensionSourceResult { changed: false });
        }
        Ok(_) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => {
            return Err(DiagnosticSet::one(diag(
                code,
                format!(
                    "failed to read dimension source `{}`: {err}",
                    path.display()
                ),
            )));
        }
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            DiagnosticSet::one(diag(
                code,
                format!("failed to create `{}`: {err}", parent.display()),
            ))
        })?;
    }
    fs::write(path, body).map_err(|err| {
        DiagnosticSet::one(diag(
            code,
            format!(
                "failed to write dimension source `{}`: {err}",
                path.display()
            ),
        ))
    })?;
    Ok(DimensionSourceResult { changed: true })
}
