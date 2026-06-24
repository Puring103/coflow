//! Localization entry collection, merge with on-disk CSV, and write-back.

use crate::localization::csv;
use coflow_api::{Diagnostic, DiagnosticSet, Label, Severity, SourceLocation};
use coflow_cft::CftContainer;
use coflow_data_model::{CfdDataModel, CfdDictKey, CfdEnumValue, CfdRecord, CfdValue};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone)]
pub(super) struct Entry {
    pub type_name: String,
    pub field_name: String,
    pub is_singleton: bool,
    pub row_id: String,
    pub default: String,
}

/// Bucket coordinate (one CSV file per (type, field) for normal types, one CSV
/// per type for singletons).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct BucketKey {
    pub type_name: String,
    pub field_name: Option<String>,
}

impl BucketKey {
    pub fn file_stem(&self) -> String {
        self.field_name.as_ref().map_or_else(
            || self.type_name.clone(),
            |field| format!("{}_{field}", self.type_name),
        )
    }
}

pub(super) fn collect_entries(schema: &CftContainer, model: &CfdDataModel) -> Vec<Entry> {
    let mut out = Vec::new();
    for (_, record) in model.records() {
        let Some(schema_type) = schema.resolve_type(&record.actual_type) else {
            continue;
        };
        let is_singleton = schema_type.is_singleton;
        for field in &schema_type.all_fields {
            if !field.is_localized {
                continue;
            }
            let value = record.fields.get(&field.name);
            let default = value.map_or_else(String::new, render_value);
            let row_id = if is_singleton {
                field.name.clone()
            } else {
                record.key().to_string()
            };
            out.push(Entry {
                type_name: record.actual_type.clone(),
                field_name: field.name.clone(),
                is_singleton,
                row_id,
                default,
            });
        }
    }
    out
}

fn render_value(value: &CfdValue) -> String {
    match value {
        CfdValue::Null => String::new(),
        CfdValue::Bool(b) => b.to_string(),
        CfdValue::Int(i) => i.to_string(),
        CfdValue::Float(f) => f.to_string(),
        CfdValue::String(s) => s.clone(),
        CfdValue::Enum(e) => format_enum(e),
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
                .map(|(k, v)| format!("{}: {}", format_dict_key(k), render_value(v)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{{{inner}}}")
        }
    }
}

fn format_enum(e: &CfdEnumValue) -> String {
    e.variant.as_deref().map_or_else(
        || format!("{}({})", e.enum_name, e.value),
        |v| format!("{}.{}", e.enum_name, v),
    )
}

fn format_dict_key(key: &CfdDictKey) -> String {
    match key {
        CfdDictKey::String(s) => format!("\"{s}\""),
        CfdDictKey::Int(i) => i.to_string(),
        CfdDictKey::Enum(e) => format_enum(e),
    }
}

fn format_object(record: &CfdRecord) -> String {
    let inner = record
        .fields
        .iter()
        .map(|(k, v)| format!("{k}: {}", render_value(v)))
        .collect::<Vec<_>>()
        .join(", ");
    format!("{{{inner}}}")
}

/// In-memory representation of one bucket's CSV.
#[derive(Debug, Clone)]
pub(super) struct BucketTable {
    pub rows: BTreeMap<String, BucketRow>,
}

#[derive(Debug, Clone, Default)]
pub(super) struct BucketRow {
    pub default: String,
    pub translations: BTreeMap<String, String>,
}

pub(super) fn merge_with_existing(
    out_dir: &Path,
    by_bucket: BTreeMap<BucketKey, Vec<Entry>>,
    languages: &[String],
    config_path: &Path,
) -> (BTreeMap<BucketKey, BucketTable>, DiagnosticSet) {
    let mut out = BTreeMap::new();
    let mut diagnostics = DiagnosticSet::empty();
    for (bucket, entries) in by_bucket {
        let mut rows: BTreeMap<String, BucketRow> = BTreeMap::new();
        for entry in entries {
            let row = rows.entry(entry.row_id).or_default();
            row.default = entry.default;
        }
        let path = out_dir.join(format!("{}.csv", bucket.file_stem()));
        match fs::read_to_string(&path) {
            Ok(text) => match csv::parse(&text) {
                Ok(parsed) => merge_existing_rows(&parsed, &mut rows, languages),
                Err(err) => diagnostics.push(parse_diagnostic(
                    config_path,
                    format!(
                        "failed to parse localization table `{}`: {err}",
                        path.display()
                    ),
                )),
            },
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => diagnostics.push(parse_diagnostic(
                config_path,
                format!(
                    "failed to read localization table `{}`: {err}",
                    path.display()
                ),
            )),
        }
        out.insert(bucket, BucketTable { rows });
    }
    (out, diagnostics)
}

fn merge_existing_rows(
    parsed: &[Vec<String>],
    rows: &mut BTreeMap<String, BucketRow>,
    languages: &[String],
) {
    let Some(header) = parsed.first() else {
        return;
    };
    let mut lang_columns: BTreeMap<String, usize> = BTreeMap::new();
    for (col, name) in header.iter().enumerate() {
        if name == "id" || name == "default" {
            continue;
        }
        if languages.iter().any(|lang| lang == name) {
            lang_columns.insert(name.clone(), col);
        }
    }
    let Some(id_col) = header.iter().position(|h| h == "id") else {
        return;
    };
    for record in parsed.iter().skip(1) {
        let Some(id) = record.get(id_col) else {
            continue;
        };
        let Some(row) = rows.get_mut(id) else {
            continue;
        };
        for (lang, col) in &lang_columns {
            if let Some(cell) = record.get(*col) {
                if !cell.is_empty() {
                    row.translations.insert(lang.clone(), cell.clone());
                }
            }
        }
    }
}

pub(super) fn write_buckets(
    out_dir: &Path,
    buckets: BTreeMap<BucketKey, BucketTable>,
    languages: &[String],
    config_path: &Path,
) -> DiagnosticSet {
    let mut diagnostics = DiagnosticSet::empty();
    if buckets.is_empty() {
        return diagnostics;
    }
    if let Err(err) = fs::create_dir_all(out_dir) {
        diagnostics.push(write_diagnostic(
            config_path,
            format!(
                "failed to create localization out_dir `{}`: {err}",
                out_dir.display()
            ),
        ));
        return diagnostics;
    }
    for (bucket, table) in buckets {
        let path = out_dir.join(format!("{}.csv", bucket.file_stem()));
        let mut rows: Vec<Vec<String>> = Vec::new();
        let mut header = vec!["id".to_string(), "default".to_string()];
        header.extend(languages.iter().cloned());
        rows.push(header);
        for (id, row) in table.rows {
            let mut record = vec![id, row.default];
            for lang in languages {
                let cell = row.translations.get(lang).cloned().unwrap_or_default();
                record.push(cell);
            }
            rows.push(record);
        }
        let body = csv::write(&rows);
        if let Err(err) = fs::write(&path, body) {
            diagnostics.push(write_diagnostic(
                config_path,
                format!(
                    "failed to write localization table `{}`: {err}",
                    path.display()
                ),
            ));
        }
    }
    diagnostics
}

fn parse_diagnostic(config_path: &Path, message: String) -> Diagnostic {
    loc_diagnostic(config_path, "LOC-IO-001", message)
}

fn write_diagnostic(config_path: &Path, message: String) -> Diagnostic {
    loc_diagnostic(config_path, "LOC-IO-003", message)
}

fn loc_diagnostic(config_path: &Path, code: &str, message: String) -> Diagnostic {
    Diagnostic {
        code: code.to_string(),
        stage: "LOCALIZATION".to_string(),
        severity: Severity::Error,
        message,
        primary: Some(Label {
            location: SourceLocation::ProjectConfig {
                path: config_path.to_path_buf(),
                key_path: vec!["localization".to_string()],
            },
            message: None,
        }),
        related: Vec::new(),
    }
}
