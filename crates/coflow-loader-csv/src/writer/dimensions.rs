use coflow_api::{
    DecodedSourceOptions, Diagnostic, DiagnosticSet, DimensionSourceLoadRequest,
    DimensionSourceLoadResult, DimensionSourceManager, DimensionSourceManagerDescriptor,
    DimensionSourceOptionsRequest, DimensionSourceRequest, DimensionSourceResult,
    RewriteDimensionRecordRequest, SourceLocationSpec, TableContext, WriteDimensionValueRequest,
};
use coflow_cft::{CftSchemaTypeRef, RecordKey};
use coflow_data_model::{
    CfdDictKey, CfdInputDimensionValue, CfdValue, RecordOrigin, SourceDocument,
};
use coflow_loader_table_core::cell_value::{
    parse_schema_cell, render_cell_value, CellRenderError, ParsedCell,
};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
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

    fn load_dimension_source(
        &self,
        _ctx: TableContext<'_>,
        request: &DimensionSourceLoadRequest<'_>,
    ) -> Result<DimensionSourceLoadResult, DiagnosticSet> {
        let SourceLocationSpec::Path(path) = &request.source.location;
        let text = fs::read_to_string(path).map_err(|err| {
            DiagnosticSet::one(diag(
                "CSV-DIMENSION",
                format!(
                    "failed to read dimension source `{}`: {err}",
                    path.display()
                ),
            ))
        })?;
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
            return Ok(DimensionSourceLoadResult::default());
        };
        let Some(id_column) = header.iter().position(|name| name == "id") else {
            return Err(DiagnosticSet::one(diag(
                "CSV-DIMENSION",
                "dimension CSV requires an `id` column",
            )));
        };
        let variant_columns = request
            .schema
            .dimension
            .variants
            .iter()
            .filter_map(|variant| {
                header
                    .iter()
                    .position(|name| name == variant.as_str())
                    .map(|column| (variant, column))
            })
            .collect::<Vec<_>>();
        let nullable_type = CftSchemaTypeRef::Nullable(Box::new(
            request.schema.source_field.ty_ref.non_nullable().clone(),
        ));
        let mut values = Vec::new();
        let mut diagnostics = DiagnosticSet::empty();
        for (row_index, row) in rows.iter().enumerate().skip(1) {
            let Some(raw_key) = row.get(id_column).filter(|key| !key.trim().is_empty()) else {
                continue;
            };
            let source_key = match RecordKey::new(raw_key.clone()) {
                Ok(key) => key,
                Err(err) => {
                    diagnostics.push(Diagnostic::error("CSV-DIMENSION", "CSV", err.to_string()));
                    continue;
                }
            };
            for (variant, column) in &variant_columns {
                let Some(text) = row.get(*column) else {
                    continue;
                };
                let parsed = match parse_schema_cell(request.schema.schema, &nullable_type, text) {
                    Ok(parsed) => parsed,
                    Err(err) => {
                        diagnostics.push(Diagnostic::error(
                            "CSV-DIMENSION-VALUE",
                            "CSV",
                            err.diagnostics
                                .into_iter()
                                .map(|item| item.message)
                                .collect::<Vec<_>>()
                                .join("; "),
                        ));
                        continue;
                    }
                };
                let ParsedCell::Value(value) = parsed else {
                    continue;
                };
                values.push(CfdInputDimensionValue {
                    source_type: request.schema.source_type.name.clone(),
                    source_key: source_key.clone(),
                    field: request.schema.source_field.name.clone(),
                    dimension: request.schema.dimension.name.clone(),
                    variant: (*variant).clone(),
                    value,
                    origin: RecordOrigin::Table {
                        document: SourceDocument::Local(path.clone()),
                        sheet: request.source.display_name.clone(),
                        row: row_index,
                        id_column,
                        field_columns: std::iter::once((Vec::new(), *column)).collect(),
                    },
                });
            }
        }
        if diagnostics.is_empty() {
            Ok(DimensionSourceLoadResult { values })
        } else {
            Err(diagnostics)
        }
    }

    fn source_options(
        &self,
        request: &DimensionSourceOptionsRequest<'_>,
    ) -> Result<DecodedSourceOptions, DiagnosticSet> {
        crate::options::decode_csv_source_options(&json!({
            "sheets": [{
                "sheet": request.sheet,
                "type": request.actual_type,
            }]
        }))
    }

    fn write_dimension_value(
        &self,
        _ctx: TableContext<'_>,
        request: &WriteDimensionValueRequest<'_>,
    ) -> Result<DimensionSourceResult, DiagnosticSet> {
        let SourceLocationSpec::Path(path) = &request.source.location;
        let text = fs::read_to_string(path).map_err(|err| {
            DiagnosticSet::one(diag(
                "CSV-DIMENSION-WRITE",
                format!(
                    "failed to read dimension source `{}`: {err}",
                    path.display()
                ),
            ))
        })?;
        let mut rows = parse(&text).map_err(|err| {
            DiagnosticSet::one(diag(
                "CSV-DIMENSION-WRITE",
                format!(
                    "failed to parse dimension source `{}`: {err}",
                    path.display()
                ),
            ))
        })?;
        let Some(header) = rows.first() else {
            return Err(DiagnosticSet::one(diag(
                "CSV-DIMENSION-WRITE",
                "dimension CSV is empty",
            )));
        };
        let id_column = header.iter().position(|name| name == "id").ok_or_else(|| {
            DiagnosticSet::one(diag(
                "CSV-DIMENSION-WRITE",
                "dimension CSV requires an `id` column",
            ))
        })?;
        let variant_column = header
            .iter()
            .position(|name| name == request.variant.as_str())
            .ok_or_else(|| {
                DiagnosticSet::one(diag(
                    "CSV-DIMENSION-WRITE",
                    format!("unknown dimension variant `{}`", request.variant),
                ))
            })?;
        let header_len = header.len();
        let matching_rows = rows
            .iter()
            .enumerate()
            .skip(1)
            .filter(|(_, row)| {
                row.get(id_column)
                    .is_some_and(|key| key == request.source_key.as_str())
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        let [row_index] = matching_rows.as_slice() else {
            return Err(DiagnosticSet::one(diag(
                "CSV-DIMENSION-WRITE",
                format!(
                    "dimension source requires exactly one row for `{}`, found {}",
                    request.source_key,
                    matching_rows.len()
                ),
            )));
        };
        let row = &mut rows[*row_index];
        row.resize(header_len, String::new());
        row[variant_column] = match request.new_value {
            None => String::new(),
            Some(CfdValue::Null) => "null".to_string(),
            Some(value) => render_dimension_csv_value(value),
        };
        let body = write(&rows);
        write_if_changed(path, &body, "CSV-DIMENSION-WRITE")
    }

    fn rewrite_dimension_record(
        &self,
        _ctx: TableContext<'_>,
        request: &RewriteDimensionRecordRequest<'_>,
    ) -> Result<DimensionSourceResult, DiagnosticSet> {
        let SourceLocationSpec::Path(path) = &request.source.location;
        let text = fs::read_to_string(path).map_err(|err| {
            DiagnosticSet::one(diag(
                "CSV-DIMENSION-WRITE",
                format!(
                    "failed to read dimension source `{}`: {err}",
                    path.display()
                ),
            ))
        })?;
        let mut rows = parse(&text).map_err(|err| {
            DiagnosticSet::one(diag(
                "CSV-DIMENSION-WRITE",
                format!(
                    "failed to parse dimension source `{}`: {err}",
                    path.display()
                ),
            ))
        })?;
        let Some(header) = rows.first() else {
            return Err(DiagnosticSet::one(diag(
                "CSV-DIMENSION-WRITE",
                "dimension CSV is empty",
            )));
        };
        let id_column = header.iter().position(|name| name == "id").ok_or_else(|| {
            DiagnosticSet::one(diag(
                "CSV-DIMENSION-WRITE",
                "dimension CSV requires an `id` column",
            ))
        })?;
        let header_len = header.len();
        let matching_rows = rows
            .iter()
            .enumerate()
            .skip(1)
            .filter(|(_, row)| {
                row.get(id_column)
                    .is_some_and(|key| key == request.old_key.as_str())
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        let [row_index] = matching_rows.as_slice() else {
            return Err(DiagnosticSet::one(diag(
                "CSV-DIMENSION-WRITE",
                format!(
                    "dimension source requires exactly one row for `{}`, found {}",
                    request.old_key,
                    matching_rows.len()
                ),
            )));
        };
        if let Some(new_key) = request.new_key {
            rows[*row_index].resize(header_len, String::new());
            rows[*row_index][id_column] = new_key.to_string();
        } else {
            rows.remove(*row_index);
        }
        write_if_changed(path, &write(&rows), "CSV-DIMENSION-WRITE")
    }

    fn sync_dimension_source(
        &self,
        _ctx: TableContext<'_>,
        request: &DimensionSourceRequest<'_>,
    ) -> Result<DimensionSourceResult, DiagnosticSet> {
        let SourceLocationSpec::Path(path) = &request.source.location;
        let expected_keys = request
            .entries
            .iter()
            .map(|entry| entry.key.as_str())
            .collect::<BTreeSet<_>>();
        let existing = read_existing_dimension_csv(path, request.variants, &expected_keys)?;
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
                        .map_or_else(|| "null".to_string(), Clone::clone),
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
    expected_keys: &BTreeSet<&str>,
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
        if !expected_keys.contains(id.as_str()) {
            return Err(DiagnosticSet::one(diag(
                "CSV-DIMENSION",
                format!(
                    "dimension source `{}` contains unmanaged id `{id}`; variant tables can only edit existing records",
                    path.display()
                ),
            )));
        }
        if out.contains_key(id) {
            return Err(DiagnosticSet::one(diag(
                "CSV-DIMENSION",
                format!(
                    "dimension source `{}` contains duplicate id `{id}`; variant tables can only edit existing records",
                    path.display()
                ),
            )));
        }
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

fn render_dimension_csv_value(value: &CfdValue) -> String {
    match render_cell_value(value) {
        Ok(value) => value,
        Err(CellRenderError::NestedObject | CellRenderError::AnonymousEnum) => {
            render_fallback_cell_value(value)
        }
    }
}

fn render_fallback_cell_value(value: &CfdValue) -> String {
    match value {
        CfdValue::Null => String::new(),
        CfdValue::Bool(value) => value.to_string(),
        CfdValue::Int(value) => value.to_string(),
        CfdValue::Float(value) => value.to_string(),
        CfdValue::String(value) => value.clone(),
        CfdValue::Enum(value) => value.variant.as_deref().map_or_else(
            || format!("{}({})", value.enum_name, value.value),
            |variant| format!("{}.{}", value.enum_name, variant),
        ),
        CfdValue::Object(record) => {
            let inner = record
                .fields()
                .iter()
                .map(|(key, value)| format!("{key}: {}", render_fallback_cell_value(value)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{{{inner}}}")
        }
        CfdValue::Ref(target_key) => format!("&{target_key}"),
        CfdValue::Array(items) => {
            let inner = items
                .iter()
                .map(render_fallback_cell_value)
                .collect::<Vec<_>>()
                .join(", ");
            format!("[{inner}]")
        }
        CfdValue::Dict(entries) => {
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

fn render_fallback_dict_key(key: &CfdDictKey) -> String {
    match key {
        CfdDictKey::String(value) => format!("{value:?}"),
        CfdDictKey::Int(value) => value.to_string(),
        CfdDictKey::Enum(value) => value.variant.as_deref().map_or_else(
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
