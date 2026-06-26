use crate::dimensions::DimensionField;
use coflow_api::{Diagnostic, DiagnosticSet, Label, Severity, SourceLocation};
use coflow_cfd::{parse_cfd, CfdBlockEntry};
use coflow_data_model::{CfdDataModel, CfdDictKey, CfdEnumValue, CfdRecord, CfdValue};
use coflow_loader_csv as csv;
use coflow_loader_table_core::cell_value::{render_cell_value, CellRenderError};
use coflow_project::Project;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

#[must_use]
pub fn regenerate_dimension_sources(
    project: &Project,
    model: &CfdDataModel,
    fields: &[DimensionField],
) -> DiagnosticSet {
    let Some(config) = project.config.dimensions.get("language") else {
        return DiagnosticSet::empty();
    };
    let Some(out_dir) = config.out_dir.as_ref() else {
        return DiagnosticSet::one(dimension_diagnostic(
            &project.config_path,
            "DIM-CONFIG-003",
            "dimensions.language.out_dir is required",
        ));
    };
    let out_dir = project.resolve_path(out_dir);
    let mut diagnostics = DiagnosticSet::empty();
    if let Err(err) = fs::create_dir_all(&out_dir) {
        diagnostics.push(dimension_diagnostic(
            &project.config_path,
            "DIM-SOURCE-001",
            format!(
                "failed to create dimension out_dir `{}`: {err}",
                out_dir.display()
            ),
        ));
        return diagnostics;
    }

    for field in fields {
        if field.is_singleton {
            diagnostics.extend(regenerate_singleton_file(
                project,
                model,
                field,
                &out_dir,
                &config.variants,
            ));
        } else {
            diagnostics.extend(regenerate_csv_file(
                project,
                model,
                field,
                &out_dir,
                &config.variants,
            ));
        }
    }
    diagnostics
}

fn regenerate_csv_file(
    project: &Project,
    model: &CfdDataModel,
    field: &DimensionField,
    out_dir: &Path,
    variants: &[String],
) -> DiagnosticSet {
    let path = out_dir.join(format!("{}_{}.csv", field.bucket, field.source_field));
    let mut existing = match read_existing_csv(&path, &project.config_path, variants) {
        Ok(existing) => existing,
        Err(diagnostics) => return diagnostics,
    };
    for (_, record) in model.records_of_type(&field.source_type) {
        let row = existing.entry(record.key().to_string()).or_default();
        row.default = record
            .fields
            .get(&field.source_field)
            .map_or_else(String::new, render_csv_value);
    }

    let mut rows = Vec::new();
    let mut header = vec!["id".to_string(), "default".to_string()];
    header.extend(variants.iter().cloned());
    rows.push(header);
    for (id, row) in existing {
        let mut record = vec![id, row.default];
        for variant in variants {
            record.push(
                row.variants
                    .get(variant)
                    .map_or_else(|| "null".to_string(), |value| csv_variant_cell(value)),
            );
        }
        rows.push(record);
    }
    write_file(&path, csv::write(&rows), &project.config_path)
}

fn csv_variant_cell(value: &str) -> String {
    if value.is_empty() {
        "null".to_string()
    } else {
        value.to_string()
    }
}

fn regenerate_singleton_file(
    project: &Project,
    model: &CfdDataModel,
    field: &DimensionField,
    out_dir: &Path,
    variants: &[String],
) -> DiagnosticSet {
    let path = out_dir.join(format!("{}.cfd", field.source_type));
    let mut existing = match read_existing_singleton(&path, &project.config_path, variants) {
        Ok(existing) => existing,
        Err(diagnostics) => return diagnostics,
    };
    if let Some((_, record)) = model.records_of_type(&field.source_type).next() {
        let row = existing.entry(field.source_field.clone()).or_default();
        row.actual_type.clone_from(&field.synthesized_type);
        row.default = record
            .fields
            .get(&field.source_field)
            .map_or_else(|| "null".to_string(), render_cfd_value);
    }

    let mut out = String::new();
    for (field_name, row) in existing {
        let actual_type = if row.actual_type.is_empty() {
            &field.synthesized_type
        } else {
            &row.actual_type
        };
        let _ = writeln!(out, "{field_name}: {actual_type} {{");
        let _ = writeln!(out, "    default: {},", render_cfd_cell(&row.default));
        for variant in variants {
            let value = row.variants.get(variant).cloned().unwrap_or_default();
            let _ = writeln!(out, "    {variant}: {},", render_cfd_cell(&value));
        }
        out.push_str("}\n\n");
    }
    write_file(&path, out, &project.config_path)
}

#[derive(Debug, Clone, Default)]
struct DimensionRow {
    actual_type: String,
    default: String,
    variants: BTreeMap<String, String>,
}

fn read_existing_csv(
    path: &Path,
    config_path: &Path,
    variants: &[String],
) -> Result<BTreeMap<String, DimensionRow>, DiagnosticSet> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(err) => {
            return Err(DiagnosticSet::one(dimension_diagnostic(
                config_path,
                "DIM-SOURCE-001",
                format!(
                    "failed to read dimension source `{}`: {err}",
                    path.display()
                ),
            )));
        }
    };
    let rows = match csv::parse(&text) {
        Ok(rows) => rows,
        Err(err) => {
            return Err(DiagnosticSet::one(dimension_diagnostic(
                config_path,
                "DIM-SOURCE-001",
                format!(
                    "failed to parse dimension source `{}`: {err}",
                    path.display()
                ),
            )));
        }
    };
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
        let row = out.entry(id.clone()).or_insert_with(DimensionRow::default);
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

fn read_existing_singleton(
    path: &Path,
    config_path: &Path,
    variants: &[String],
) -> Result<BTreeMap<String, DimensionRow>, DiagnosticSet> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(err) => {
            return Err(DiagnosticSet::one(dimension_diagnostic(
                config_path,
                "DIM-SOURCE-001",
                format!(
                    "failed to read dimension source `{}`: {err}",
                    path.display()
                ),
            )));
        }
    };

    let (ast, diagnostics) = parse_cfd(&text);
    if let Some(diagnostic) = diagnostics.first() {
        return Err(DiagnosticSet::one(dimension_diagnostic(
            config_path,
            "DIM-SOURCE-001",
            format!(
                "failed to parse dimension source `{}`: {}",
                path.display(),
                diagnostic.message
            ),
        )));
    }

    let mut out = BTreeMap::new();
    for record in ast.records {
        let mut row = DimensionRow {
            actual_type: record.type_name,
            ..DimensionRow::default()
        };
        for entry in record.entries {
            let CfdBlockEntry::Field(field) = entry else {
                continue;
            };
            let value = raw_value_text(&text, field.value.span()).unwrap_or_default();
            if field.name == "default" {
                row.default = value;
            } else if variants.iter().any(|variant| variant == &field.name) {
                row.variants.insert(field.name, value);
            }
        }
        out.insert(record.key, row);
    }
    Ok(out)
}

fn write_file(path: &Path, body: String, config_path: &Path) -> DiagnosticSet {
    match fs::write(path, body) {
        Ok(()) => DiagnosticSet::empty(),
        Err(err) => DiagnosticSet::one(dimension_diagnostic(
            config_path,
            "DIM-SOURCE-001",
            format!(
                "failed to write dimension source `{}`: {err}",
                path.display()
            ),
        )),
    }
}

fn render_cfd_cell(value: &str) -> String {
    if value.is_empty() {
        "null".to_string()
    } else {
        value.to_string()
    }
}

fn raw_value_text(source: &str, span: coflow_cft::Span) -> Option<String> {
    source
        .get(span.start..span.end)
        .map(str::trim)
        .map(str::to_string)
}

fn render_value(value: &CfdValue) -> String {
    match value {
        CfdValue::Null => String::new(),
        CfdValue::Bool(value) => value.to_string(),
        CfdValue::Int(value) => value.to_string(),
        CfdValue::Float(value) => value.to_string(),
        CfdValue::String(value) => value.clone(),
        CfdValue::Enum(value) => format_enum(value),
        CfdValue::Object(record) => format_object(record),
        CfdValue::Ref { key, .. } => format!("&{key}"),
        CfdValue::Array(items) => {
            let inner = items
                .iter()
                .map(render_value)
                .collect::<Vec<_>>()
                .join(", ");
            format!("[{inner}]")
        }
        CfdValue::Dict(entries) => {
            let inner = entries
                .iter()
                .map(|(key, value)| format!("{}: {}", format_dict_key(key), render_value(value)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{{{inner}}}")
        }
    }
}

fn render_csv_value(value: &CfdValue) -> String {
    match render_cell_value(value) {
        Ok(value) => value,
        Err(CellRenderError::NestedObject | CellRenderError::AnonymousEnum) => render_value(value),
    }
}

fn render_cfd_value(value: &CfdValue) -> String {
    match value {
        CfdValue::Null => "null".to_string(),
        CfdValue::Bool(value) => value.to_string(),
        CfdValue::Int(value) => value.to_string(),
        CfdValue::Float(value) => {
            let text = value.to_string();
            if text.contains('.') || text.contains('e') || text.contains('E') {
                text
            } else {
                format!("{text}.0")
            }
        }
        CfdValue::String(value) => format!("{value:?}"),
        CfdValue::Enum(value) => value
            .variant
            .clone()
            .unwrap_or_else(|| format!("{}({})", value.enum_name, value.value)),
        CfdValue::Object(record) => format_cfd_object(record),
        CfdValue::Ref { key, .. } => format!("&{key}"),
        CfdValue::Array(items) => {
            let inner = items
                .iter()
                .map(render_cfd_value)
                .collect::<Vec<_>>()
                .join(", ");
            format!("[{inner}]")
        }
        CfdValue::Dict(entries) => {
            let inner = entries
                .iter()
                .map(|(key, value)| {
                    format!("{}: {}", format_cfd_dict_key(key), render_cfd_value(value))
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("{{{inner}}}")
        }
    }
}

fn format_enum(value: &CfdEnumValue) -> String {
    value.variant.as_deref().map_or_else(
        || format!("{}({})", value.enum_name, value.value),
        |variant| format!("{}.{}", value.enum_name, variant),
    )
}

fn format_dict_key(key: &CfdDictKey) -> String {
    match key {
        CfdDictKey::String(value) => format!("{value:?}"),
        CfdDictKey::Int(value) => value.to_string(),
        CfdDictKey::Enum(value) => format_enum(value),
    }
}

fn format_object(record: &CfdRecord) -> String {
    let inner = record
        .fields
        .iter()
        .map(|(key, value)| format!("{key}: {}", render_value(value)))
        .collect::<Vec<_>>()
        .join(", ");
    format!("{{{inner}}}")
}

fn format_cfd_object(record: &CfdRecord) -> String {
    let inner = record
        .fields
        .iter()
        .map(|(key, value)| format!("{key}: {}", render_cfd_value(value)))
        .collect::<Vec<_>>()
        .join(", ");
    format!("{} {{{inner}}}", record.actual_type)
}

fn format_cfd_dict_key(key: &CfdDictKey) -> String {
    match key {
        CfdDictKey::String(value) => format!("{value:?}"),
        CfdDictKey::Int(value) => value.to_string(),
        CfdDictKey::Enum(value) => value
            .variant
            .clone()
            .unwrap_or_else(|| format!("{}({})", value.enum_name, value.value)),
    }
}

fn dimension_diagnostic(config_path: &Path, code: &str, message: impl Into<String>) -> Diagnostic {
    Diagnostic {
        code: code.to_string(),
        stage: "PROJECT".to_string(),
        severity: Severity::Error,
        message: message.into(),
        primary: Some(Label {
            location: SourceLocation::ProjectConfig {
                path: config_path.to_path_buf(),
                key_path: vec!["dimensions".to_string(), "language".to_string()],
            },
            message: None,
        }),
        related: Vec::new(),
    }
}
