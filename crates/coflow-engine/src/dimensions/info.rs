//! Resolved dimension metadata exposed to hosts.
//!
//! Combines the `DimensionConfig` declared in `coflow.yaml` with the schema
//! dimension fields discovered during model build.

use coflow_project::{DimensionConfig, Project};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use super::synthesize::DimensionField;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
pub struct DimensionInfo {
    /// Stable dimension name from `coflow.yaml` (e.g. `"language"`).
    pub name: String,
    /// Human-readable label resolved with the `display_name` fallback chain:
    /// `config.display_name` → built-in (`"language" → "本地化"`) → `name`.
    pub display_name: String,
    pub variants: Vec<String>,
    /// Output directory (project-relative path string) for synthesized
    /// dimension records, or `None` when no `out_dir` is configured.
    pub out_dir: Option<String>,
    /// Schema fields belonging to this dimension. Wire only the source
    /// type/field/synthesized type — the schema view itself is not part of
    /// the editor's surface.
    pub fields: Vec<DimensionFieldInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
pub struct DimensionFieldInfo {
    pub source_type: String,
    pub source_field: String,
    pub synthesized_type: String,
    pub is_singleton: bool,
}

/// Resolve all configured dimensions into `DimensionInfo`. Synthetic fields
/// passed in are typically the result of `dimension_fields(schema)`.
#[must_use]
pub fn dimensions_for_project(project: &Project, fields: &[DimensionField]) -> Vec<DimensionInfo> {
    let mut by_name: std::collections::BTreeMap<&str, Vec<&DimensionField>> =
        std::collections::BTreeMap::new();
    for field in fields {
        by_name
            .entry(field.dimension.as_str())
            .or_default()
            .push(field);
    }

    let mut out = Vec::new();
    for (name, config) in &project.config.dimensions {
        let display_name = resolved_display_name(name, config);
        let out_dir = config.out_dir.as_ref().map(|p: &PathBuf| {
            let absolute = project.resolve_path(p);
            let rel = absolute
                .strip_prefix(&project.root_dir)
                .unwrap_or(&absolute);
            rel.to_string_lossy().replace('\\', "/")
        });
        let info_fields = by_name
            .get(name.as_str())
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|field| DimensionFieldInfo {
                source_type: field.source_type.clone(),
                source_field: field.source_field.clone(),
                synthesized_type: field.synthesized_type.clone(),
                is_singleton: field.is_singleton,
            })
            .collect();
        out.push(DimensionInfo {
            name: name.clone(),
            display_name,
            variants: config.variants.clone(),
            out_dir,
            fields: info_fields,
        });
    }
    out
}

/// Compute the display label for a dimension. Falls back through:
/// 1. `config.display_name` (explicit user choice),
/// 2. a small built-in table for shipped dimensions,
/// 3. the raw `name`.
#[must_use]
pub fn resolved_display_name(name: &str, config: &DimensionConfig) -> String {
    if let Some(custom) = &config.display_name {
        return custom.clone();
    }
    builtin_display_name(name).map_or_else(|| name.to_string(), str::to_string)
}

#[must_use]
pub const fn builtin_display_name(name: &str) -> Option<&'static str> {
    match name.as_bytes() {
        b"language" => Some("本地化"),
        _ => None,
    }
}
