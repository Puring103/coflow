//! Translation table generation for `@localized` schema fields.
//!
//! See `docs/spec/13-localization.md` for the design.
//!
//! Pipeline integration: `generate_localization_tables` is called from
//! `build_project_session` after `CfdDataModel` build succeeds. It walks the
//! model, collects `(bucket, key, default_value)` triples for every
//! `@localized` field instance, merges with existing on-disk CSVs (preserving
//! human-edited translation columns), and writes CSV files back to
//! `<localization.out_dir>/<bucket>.csv`.

mod key;
mod tables;

use crate::localization::tables::{collect_entries, merge_with_existing, write_buckets, BucketKey};
use coflow_api::{Diagnostic, DiagnosticSet, Label, Severity, SourceLocation};
use coflow_data_model::CfdDataModel;
use coflow_loader_csv as csv;
use coflow_project::{LocalizationConfig, Project};
use std::path::{Path, PathBuf};

pub use crate::localization::key::{format_key, LocalizationKey};

use coflow_checker::LocalizationOverrides;
use std::collections::BTreeMap;
use std::fs;

/// Generate translation tables for a project. No-op when localization is not
/// configured. Returns diagnostics for any IO errors.
#[must_use]
pub fn generate_localization_tables(
    project: &Project,
    schema: &coflow_cft::CftContainer,
    model: &CfdDataModel,
) -> DiagnosticSet {
    let Some(config) = &project.config.localization else {
        return DiagnosticSet::empty();
    };
    let out_dir = resolve_out_dir(project, config);
    let entries = collect_entries(schema, model);
    let buckets = group_by_bucket(entries);
    let (merged, mut diagnostics) =
        merge_with_existing(&out_dir, buckets, &config.languages, &project.config_path);
    let write_diagnostics =
        write_buckets(&out_dir, merged, &config.languages, &project.config_path);
    diagnostics.extend(write_diagnostics);
    diagnostics
}

fn resolve_out_dir(project: &Project, config: &LocalizationConfig) -> PathBuf {
    if config.out_dir.is_absolute() {
        config.out_dir.clone()
    } else {
        project.root_dir.join(&config.out_dir)
    }
}

fn group_by_bucket(entries: Vec<tables::Entry>) -> BTreeMap<BucketKey, Vec<tables::Entry>> {
    let mut out: BTreeMap<BucketKey, Vec<tables::Entry>> = BTreeMap::new();
    for entry in entries {
        let bucket = BucketKey {
            type_name: entry.type_name.clone(),
            field_name: if entry.is_singleton {
                None
            } else {
                Some(entry.field_name.clone())
            },
        };
        out.entry(bucket).or_default().push(entry);
    }
    out
}

/// Result of loading per-language overrides from disk.
#[derive(Debug)]
pub struct LoadedOverrides {
    pub overrides: Vec<LocalizationOverrides>,
    pub diagnostics: DiagnosticSet,
}

/// Builds one [`LocalizationOverrides`] per declared language by reading the
/// freshly-generated CSV translation tables under `localization.out_dir`.
///
/// CSV parse failures surface as `LOC-IO-001` diagnostics. Per-language CSV
/// columns that are empty fall back to the default value at check time.
#[must_use]
pub fn load_overrides_for_languages(
    project: &Project,
    schema: &coflow_cft::CftContainer,
    config: &LocalizationConfig,
) -> LoadedOverrides {
    let mut diagnostics = DiagnosticSet::empty();
    if config.languages.is_empty() || !schema_has_localized_field(schema) {
        return LoadedOverrides {
            overrides: Vec::new(),
            diagnostics,
        };
    }
    let out_dir = resolve_out_dir(project, config);
    let mut per_lang: BTreeMap<String, BTreeMap<String, String>> = config
        .languages
        .iter()
        .map(|l| (l.clone(), BTreeMap::new()))
        .collect();
    if !out_dir.exists() {
        return LoadedOverrides {
            overrides: Vec::new(),
            diagnostics,
        };
    }
    for schema_type in schema.all_types() {
        let is_singleton = schema_type.is_singleton;
        for field in &schema_type.all_fields {
            if !field.is_localized {
                continue;
            }
            let (file_stem, key_prefix) = if is_singleton {
                (schema_type.name.clone(), schema_type.name.clone())
            } else {
                (
                    format!("{}_{}", schema_type.name, field.name),
                    format!("{}/{}", schema_type.name, field.name),
                )
            };
            let path = out_dir.join(format!("{file_stem}.csv"));
            if !path.exists() {
                continue;
            }
            load_one_csv(
                &path,
                &project.config_path,
                config,
                &key_prefix,
                &mut per_lang,
                &mut diagnostics,
            );
        }
    }
    let overrides = config
        .languages
        .iter()
        .map(|lang| LocalizationOverrides {
            language: lang.clone(),
            translations: per_lang.remove(lang).unwrap_or_default(),
        })
        .collect();
    LoadedOverrides {
        overrides,
        diagnostics,
    }
}

fn schema_has_localized_field(schema: &coflow_cft::CftContainer) -> bool {
    schema
        .all_types()
        .any(|t| t.all_fields.iter().any(|f| f.is_localized))
}

fn load_one_csv(
    path: &Path,
    config_path: &Path,
    config: &LocalizationConfig,
    key_prefix: &str,
    per_lang: &mut BTreeMap<String, BTreeMap<String, String>>,
    diagnostics: &mut DiagnosticSet,
) {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) => {
            diagnostics.push(parse_diagnostic(
                config_path,
                format!(
                    "failed to read localization table `{}`: {err}",
                    path.display()
                ),
            ));
            return;
        }
    };
    let rows = match csv::parse(&text) {
        Ok(rows) => rows,
        Err(err) => {
            diagnostics.push(parse_diagnostic(
                config_path,
                format!(
                    "failed to parse localization table `{}`: {err}",
                    path.display()
                ),
            ));
            return;
        }
    };
    let Some(header) = rows.first() else { return };
    let Some(id_col) = header.iter().position(|h| h == "id") else {
        return;
    };
    let lang_cols: BTreeMap<String, usize> = header
        .iter()
        .enumerate()
        .filter(|(_, name)| config.languages.iter().any(|l| l == *name))
        .map(|(col, name)| (name.clone(), col))
        .collect();
    for row in rows.iter().skip(1) {
        let Some(row_id) = row.get(id_col) else {
            continue;
        };
        let lookup_key = format!("{key_prefix}/{row_id}");
        for (lang, col) in &lang_cols {
            if let Some(cell) = row.get(*col) {
                if !cell.is_empty() {
                    if let Some(map) = per_lang.get_mut(lang) {
                        map.insert(lookup_key.clone(), cell.clone());
                    }
                }
            }
        }
    }
}

fn parse_diagnostic(config_path: &Path, message: String) -> Diagnostic {
    Diagnostic {
        code: "LOC-IO-001".to_string(),
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
