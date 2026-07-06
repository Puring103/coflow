use crate::dimensions::DimensionField;
use coflow_api::{Diagnostic, DiagnosticSet, Label, Severity, SourceLocation};
use coflow_cfd::{parse_cfd, CfdBlockEntry};
use coflow_data_model::{CfdDataModel, CfdDictKey, CfdEnumValue, CfdObject, CfdValue};
use coflow_loader_csv as csv;
use coflow_loader_table_core::cell_value::{render_cell_value, CellRenderError};
use coflow_project::Project;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

#[must_use]
pub fn regenerate_dimension_sources(
    project: &Project,
    model: &CfdDataModel,
    fields: &[DimensionField],
) -> DimensionGenerationResult {
    let mut diagnostics = DiagnosticSet::empty();
    let mut transaction = DimensionGenerationTransaction::default();
    for (dimension, config) in &project.config.dimensions {
        let dimension_fields = fields
            .iter()
            .filter(|field| field.dimension == *dimension)
            .collect::<Vec<_>>();
        if dimension_fields.is_empty() {
            continue;
        }
        let Some(out_dir) = config.out_dir.as_ref() else {
            diagnostics.push(dimension_diagnostic(
                &project.config_path,
                dimension,
                "DIM-CONFIG-003",
                format!("dimensions.{dimension}.out_dir is required"),
            ));
            continue;
        };
        let out_dir = project.resolve_path(out_dir);
        if let Err(err) = fs::create_dir_all(&out_dir) {
            diagnostics.push(dimension_diagnostic(
                &project.config_path,
                dimension,
                "DIM-SOURCE-001",
                format!(
                    "failed to create dimension out_dir `{}`: {err}",
                    out_dir.display()
                ),
            ));
            continue;
        }

        for field in dimension_fields {
            if field.is_singleton {
                diagnostics.extend(regenerate_singleton_file(
                    project,
                    model,
                    field,
                    &out_dir,
                    &config.variants,
                    &mut transaction,
                ));
            } else {
                diagnostics.extend(regenerate_csv_file(
                    project,
                    model,
                    field,
                    &out_dir,
                    &config.variants,
                    &mut transaction,
                ));
            }
        }
    }
    DimensionGenerationResult {
        transaction,
        diagnostics,
    }
}

#[derive(Debug, Default)]
pub struct DimensionGenerationResult {
    pub transaction: DimensionGenerationTransaction,
    pub diagnostics: DiagnosticSet,
}

#[derive(Debug, Default)]
pub struct DimensionGenerationTransaction {
    snapshots: BTreeMap<PathBuf, FileSnapshot>,
}

impl DimensionGenerationTransaction {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.snapshots.is_empty()
    }

    pub fn rollback(self, config_path: &Path) -> DiagnosticSet {
        let mut diagnostics = DiagnosticSet::empty();
        for snapshot in self.snapshots.into_values().rev() {
            if let Err(err) = snapshot.restore() {
                diagnostics.push(dimension_diagnostic(
                    config_path,
                    &snapshot.dimension,
                    "DIM-SOURCE-ROLLBACK-001",
                    format!(
                        "failed to roll back dimension source `{}`: {err}",
                        snapshot.path.display()
                    ),
                ));
            }
        }
        diagnostics
    }

    fn snapshot_file(&mut self, path: &Path, dimension: &str) {
        if self.snapshots.contains_key(path) {
            return;
        }
        let original = match fs::read_to_string(path) {
            Ok(text) => Some(text),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
            Err(_) => None,
        };
        self.snapshots.insert(
            path.to_path_buf(),
            FileSnapshot {
                path: path.to_path_buf(),
                dimension: dimension.to_string(),
                original,
            },
        );
    }
}

#[derive(Debug)]
struct FileSnapshot {
    path: PathBuf,
    dimension: String,
    original: Option<String>,
}

impl FileSnapshot {
    fn restore(&self) -> std::io::Result<()> {
        match &self.original {
            Some(text) => fs::write(&self.path, text),
            None => match fs::remove_file(&self.path) {
                Ok(()) => Ok(()),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(err) => Err(err),
            },
        }
    }
}

fn regenerate_csv_file(
    project: &Project,
    model: &CfdDataModel,
    field: &DimensionField,
    out_dir: &Path,
    variants: &[String],
    transaction: &mut DimensionGenerationTransaction,
) -> DiagnosticSet {
    let path = out_dir.join(format!("{}_{}.csv", field.bucket, field.source_field));
    let existing = match read_existing_csv(&path, &project.config_path, &field.dimension, variants)
    {
        Ok(existing) => existing,
        Err(diagnostics) => return diagnostics,
    };
    let mut generated = BTreeMap::new();
    for (_, record) in model.records_of_type(&field.source_type) {
        let mut row = existing.get(record.key()).cloned().unwrap_or_default();
        row.default = record
            .fields()
            .get(&field.source_field)
            .map_or_else(String::new, render_csv_value);
        generated.insert(record.key().to_string(), row);
    }

    let mut rows = Vec::new();
    let mut header = vec!["id".to_string(), "default".to_string()];
    header.extend(variants.iter().cloned());
    rows.push(header);
    for (id, row) in generated {
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
    write_file(
        &path,
        csv::write(&rows),
        &project.config_path,
        &field.dimension,
        transaction,
    )
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
    transaction: &mut DimensionGenerationTransaction,
) -> DiagnosticSet {
    let path = out_dir.join(format!("{}.cfd", field.source_type));
    let mut existing =
        match read_existing_singleton(&path, &project.config_path, &field.dimension, variants) {
            Ok(existing) => existing,
            Err(diagnostics) => return diagnostics,
        };
    if let Some((_, record)) = model.records_of_type(&field.source_type).next() {
        let row = existing.entry(field.source_field.clone()).or_default();
        row.actual_type.clone_from(&field.synthesized_type);
        row.default = record
            .fields()
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
    write_file(
        &path,
        out,
        &project.config_path,
        &field.dimension,
        transaction,
    )
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
    dimension: &str,
    variants: &[String],
) -> Result<BTreeMap<String, DimensionRow>, DiagnosticSet> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(err) => {
            return Err(DiagnosticSet::one(dimension_diagnostic(
                config_path,
                dimension,
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
                dimension,
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
    dimension: &str,
    variants: &[String],
) -> Result<BTreeMap<String, DimensionRow>, DiagnosticSet> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(err) => {
            return Err(DiagnosticSet::one(dimension_diagnostic(
                config_path,
                dimension,
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
            dimension,
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

fn write_file(
    path: &Path,
    body: String,
    config_path: &Path,
    dimension: &str,
    transaction: &mut DimensionGenerationTransaction,
) -> DiagnosticSet {
    match fs::read_to_string(path) {
        Ok(existing) if existing == body => return DiagnosticSet::empty(),
        Ok(_) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => {
            return DiagnosticSet::one(dimension_diagnostic(
                config_path,
                dimension,
                "DIM-SOURCE-001",
                format!(
                    "failed to read dimension source `{}`: {err}",
                    path.display()
                ),
            ));
        }
    }
    transaction.snapshot_file(path, dimension);
    match fs::write(path, body) {
        Ok(()) => DiagnosticSet::empty(),
        Err(err) => DiagnosticSet::one(dimension_diagnostic(
            config_path,
            dimension,
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
        CfdValue::Ref(target_key) => format!("&{target_key}"),
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
        CfdValue::Ref(target_key) => format!("&{target_key}"),
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

fn format_object(record: &CfdObject) -> String {
    let inner = record
        .fields()
        .iter()
        .map(|(key, value)| format!("{key}: {}", render_value(value)))
        .collect::<Vec<_>>()
        .join(", ");
    format!("{{{inner}}}")
}

fn format_cfd_object(record: &CfdObject) -> String {
    let inner = record
        .fields()
        .iter()
        .map(|(key, value)| format!("{key}: {}", render_cfd_value(value)))
        .collect::<Vec<_>>()
        .join(", ");
    format!("{} {{{inner}}}", record.actual_type())
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

fn dimension_diagnostic(
    config_path: &Path,
    dimension: &str,
    code: &str,
    message: impl Into<String>,
) -> Diagnostic {
    Diagnostic {
        code: code.to_string(),
        stage: "PROJECT".to_string(),
        severity: Severity::Error,
        message: message.into(),
        primary: Some(Label {
            location: SourceLocation::ProjectConfig {
                path: config_path.to_path_buf(),
                key_path: vec!["dimensions".to_string(), dimension.to_string()],
            },
            message: None,
        }),
        related: Vec::new(),
    }
}
