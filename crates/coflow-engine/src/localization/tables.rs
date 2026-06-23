//! Localization entry collection, merge with on-disk CSV, and write-back.

use crate::localization::csv;
use crate::localization::key::format_key;
use coflow_api::{Diagnostic, DiagnosticSet, Label, Severity, SourceLocation};
use coflow_cft::{CftContainer, CftSchemaField};
use coflow_data_model::{CfdDataModel, CfdDictKey, CfdEnumValue, CfdRecord, CfdValue};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub(crate) struct Entry {
    pub bucket: String,
    pub key: String,
    pub default: String,
}

pub(crate) fn collect_entries(schema: &CftContainer, model: &CfdDataModel) -> Vec<Entry> {
    let mut out = Vec::new();
    for (_, record) in model.records() {
        let Some(schema_type) = schema.resolve_type(&record.actual_type) else {
            continue;
        };
        for field in &schema_type.all_fields {
            if !field.is_localized {
                continue;
            }
            let bucket = field
                .localization_bucket
                .clone()
                .unwrap_or_else(|| record.actual_type.clone());
            let value = record.fields.get(&field.name);
            let default = value.map_or_else(String::new, render_value);
            let key = format_key(&bucket, record.key(), &[field.name.clone()]);
            // Recurse into nested object fields for sub-fields that are also
            // @localized. Only object fields propagate; arrays/dicts are
            // not descended per spec §2.4.
            if let Some(CfdValue::Object(nested)) = value {
                collect_nested(schema, nested, &bucket, record.key(), &[field.name.clone()], &mut out);
            }
            out.push(Entry {
                bucket,
                key,
                default,
            });
        }
    }
    out
}

fn collect_nested(
    schema: &CftContainer,
    record: &CfdRecord,
    bucket: &str,
    record_key: &str,
    parent_path: &[String],
    out: &mut Vec<Entry>,
) {
    let Some(schema_type) = schema.resolve_type(&record.actual_type) else {
        return;
    };
    for field in &schema_type.all_fields {
        if !field.is_localized {
            continue;
        }
        let mut path = parent_path.to_vec();
        path.push(field.name.clone());
        let value = record.fields.get(&field.name);
        let default = value.map_or_else(String::new, render_value);
        let key = format_key(bucket, record_key, &path);
        out.push(Entry {
            bucket: bucket.to_string(),
            key,
            default,
        });
        if let Some(CfdValue::Object(nested)) = value {
            collect_nested(schema, nested, bucket, record_key, &path, out);
        }
        let _ = field_unused(field);
    }
}

#[allow(clippy::trivially_copy_pass_by_ref)]
const fn field_unused(_: &CftSchemaField) {}

/// Render a `CfdValue` to a CSV cell string. Leaf primitives render
/// straightforwardly; composite values (array/dict/object/ref) use a JSON-like
/// notation that preserves structure. The exact form is intentionally simple
/// and stable; consumers of the CSV are expected to round-trip through this
/// module rather than parse the string with arbitrary expectations.
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
            let inner = items.iter().map(render_value).collect::<Vec<_>>().join(", ");
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
    e.variant
        .as_deref()
        .map_or_else(|| format!("{}({})", e.enum_name, e.value), |v| {
            format!("{}.{}", e.enum_name, v)
        })
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
pub(crate) struct BucketTable {
    pub languages: Vec<String>,
    /// `key -> (default, lang -> translation)`. BTreeMap so output is sorted.
    pub rows: BTreeMap<String, BucketRow>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct BucketRow {
    pub default: String,
    pub translations: BTreeMap<String, String>,
}

pub(crate) fn merge_with_existing(
    out_dir: &Path,
    by_bucket: BTreeMap<String, Vec<Entry>>,
    languages: &[String],
) -> BTreeMap<String, BucketTable> {
    let mut out = BTreeMap::new();
    for (bucket, entries) in by_bucket {
        let mut rows: BTreeMap<String, BucketRow> = BTreeMap::new();
        for entry in entries {
            let row = rows.entry(entry.key).or_default();
            row.default = entry.default;
        }
        // Try reading existing CSV.
        let path = out_dir.join(format!("{bucket}.csv"));
        if let Ok(text) = fs::read_to_string(&path) {
            if let Ok(parsed) = csv::parse(&text) {
                merge_existing_rows(parsed, &mut rows, languages);
            }
        }
        out.insert(
            bucket,
            BucketTable {
                languages: languages.to_vec(),
                rows,
            },
        );
    }
    out
}

fn merge_existing_rows(
    parsed: Vec<Vec<String>>,
    rows: &mut BTreeMap<String, BucketRow>,
    languages: &[String],
) {
    let Some(header) = parsed.first() else {
        return;
    };
    // Build lang -> column index map. Skip "key" and "default".
    let mut lang_columns: BTreeMap<String, usize> = BTreeMap::new();
    for (col, name) in header.iter().enumerate() {
        if name == "key" || name == "default" {
            continue;
        }
        if languages.iter().any(|lang| lang == name) {
            lang_columns.insert(name.clone(), col);
        }
    }
    let key_col = header.iter().position(|h| h == "key");
    let Some(key_col) = key_col else {
        return;
    };
    for record in parsed.iter().skip(1) {
        let Some(key) = record.get(key_col) else {
            continue;
        };
        // Only merge if the key still exists in the freshly collected rows.
        let Some(row) = rows.get_mut(key) else {
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

pub(crate) fn write_buckets(
    out_dir: &Path,
    buckets: BTreeMap<String, BucketTable>,
    languages: &[String],
    config_path: &Path,
) -> DiagnosticSet {
    let mut diagnostics = DiagnosticSet::empty();
    if buckets.is_empty() {
        return diagnostics;
    }
    if let Err(err) = fs::create_dir_all(out_dir) {
        diagnostics.push(io_diagnostic(
            config_path,
            format!(
                "failed to create localization out_dir `{}`: {err}",
                out_dir.display()
            ),
        ));
        return diagnostics;
    }
    for (bucket, table) in buckets {
        let path = out_dir.join(format!("{bucket}.csv"));
        let mut rows: Vec<Vec<String>> = Vec::new();
        let mut header = vec!["key".to_string(), "default".to_string()];
        header.extend(languages.iter().cloned());
        rows.push(header);
        for (key, row) in table.rows {
            let mut record = vec![key, row.default];
            for lang in languages {
                let cell = row.translations.get(lang).cloned().unwrap_or_default();
                record.push(cell);
            }
            rows.push(record);
        }
        let body = csv::write(&rows);
        if let Err(err) = fs::write(&path, body) {
            diagnostics.push(io_diagnostic(
                config_path,
                format!(
                    "failed to write localization table `{}`: {err}",
                    path.display()
                ),
            ));
        }
        let _ = bucket_used(table.languages.len());
    }
    diagnostics
}

#[allow(clippy::trivially_copy_pass_by_ref)]
const fn bucket_used(_: usize) {}

fn io_diagnostic(config_path: &Path, message: String) -> Diagnostic {
    Diagnostic {
        code: "LOC-IO-003".to_string(),
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

#[allow(dead_code)]
pub(crate) fn _output_path(out_dir: &Path, bucket: &str) -> PathBuf {
    out_dir.join(format!("{bucket}.csv"))
}
