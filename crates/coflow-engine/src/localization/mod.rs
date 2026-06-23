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

fn group_by_bucket(
    entries: Vec<tables::Entry>,
) -> std::collections::BTreeMap<String, Vec<tables::Entry>> {
    let mut out: std::collections::BTreeMap<String, Vec<tables::Entry>> =
        std::collections::BTreeMap::new();
    for entry in entries {
        out.entry(entry.bucket.clone()).or_default().push(entry);
    }
    out
}

#[allow(dead_code)]
pub(crate) fn _ensure_languages_used(_: &LocalizationConfig) {}
