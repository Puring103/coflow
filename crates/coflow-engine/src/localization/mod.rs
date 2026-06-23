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

mod csv;
mod key;
mod tables;

use crate::localization::tables::{collect_entries, merge_with_existing, write_buckets};
use coflow_api::DiagnosticSet;
use coflow_data_model::CfdDataModel;
use coflow_project::{LocalizationConfig, Project};
use std::path::PathBuf;

pub use crate::localization::key::{format_key, LocalizationKey};

use coflow_checker::LocalizationOverrides;
use std::collections::BTreeMap;
use std::fs;

/// Generate translation tables for a project. No-op when localization is not
/// configured. Returns diagnostics for any IO errors.
pub fn generate_localization_tables(
    project: &Project,
    schema: &coflow_cft::CftContainer,
    model: &CfdDataModel,
) -> DiagnosticSet {
    let Some(config) = &project.config.localization else {
        return DiagnosticSet::empty();
    };
    let out_dir: PathBuf = if config.out_dir.is_absolute() {
        config.out_dir.clone()
    } else {
        project.root_dir.join(&config.out_dir)
    };
    let entries = collect_entries(schema, model);
    let buckets = group_by_bucket(entries);
    let merged = merge_with_existing(&out_dir, buckets, &config.languages);
    write_buckets(&out_dir, merged, &config.languages, &project.config_path)
}

fn group_by_bucket(entries: Vec<tables::Entry>) -> BTreeMap<String, Vec<tables::Entry>> {
    let mut out: BTreeMap<String, Vec<tables::Entry>> = BTreeMap::new();
    for entry in entries {
        out.entry(entry.bucket.clone()).or_default().push(entry);
    }
    out
}

/// Builds one [`LocalizationOverrides`] per declared language by reading the
/// freshly-generated CSV translation tables under `localization.out_dir`.
///
/// Skipped silently when the out_dir does not yet exist (first build) or when
/// no `@localized` fields exist in the schema. Per-language CSV columns that
/// are empty fall back to the default value at check time.
pub fn load_overrides_for_languages(
    project: &Project,
    schema: &coflow_cft::CftContainer,
    config: &LocalizationConfig,
) -> Vec<LocalizationOverrides> {
    if config.languages.is_empty() {
        return Vec::new();
    }
    // No @localized fields → no translations to load.
    let any_localized = schema
        .all_types()
        .any(|t| t.all_fields.iter().any(|f| f.is_localized));
    if !any_localized {
        return Vec::new();
    }
    let out_dir = if config.out_dir.is_absolute() {
        config.out_dir.clone()
    } else {
        project.root_dir.join(&config.out_dir)
    };
    let mut per_lang: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    for lang in &config.languages {
        per_lang.insert(lang.clone(), BTreeMap::new());
    }
    let Ok(entries) = fs::read_dir(&out_dir) else {
        return Vec::new();
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("csv") {
            continue;
        }
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(rows) = csv::parse(&text) else {
            continue;
        };
        let Some(header) = rows.first() else { continue };
        let key_col = header.iter().position(|h| h == "key");
        let Some(key_col) = key_col else { continue };
        let mut lang_cols: BTreeMap<String, usize> = BTreeMap::new();
        for (col, name) in header.iter().enumerate() {
            if config.languages.iter().any(|l| l == name) {
                lang_cols.insert(name.clone(), col);
            }
        }
        for row in rows.iter().skip(1) {
            let Some(key) = row.get(key_col) else {
                continue;
            };
            for (lang, col) in &lang_cols {
                if let Some(cell) = row.get(*col) {
                    if !cell.is_empty() {
                        if let Some(map) = per_lang.get_mut(lang) {
                            map.insert(key.clone(), cell.clone());
                        }
                    }
                }
            }
        }
    }
    config
        .languages
        .iter()
        .map(|lang| LocalizationOverrides {
            language: lang.clone(),
            translations: per_lang.remove(lang).unwrap_or_default(),
        })
        .collect()
}
