//! Resolved dimension metadata exposed to hosts.
//!
//! Dimensions come from schema fields; variants come from selected CFD sources.

use crate::project::{DimensionConfig, Project};
use serde::{Deserialize, Serialize};

#[cfg(feature = "ts-export")]
use ts_rs::TS;

use coflow_language::cft::CftSchema;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
pub struct DimensionInfo {
    /// Stable dimension name declared on schema fields.
    pub name: String,
    /// Human-readable label resolved with the `display_name` fallback chain:
    /// `config.display_name` → built-in (`"language" → "本地化"`) → `name`.
    pub display_name: String,
    pub variants: Vec<String>,
    /// Schema fields belonging to this dimension. Wire only the source
    /// type and field; the schema view itself is not part of the editor surface.
    pub fields: Vec<DimensionFieldInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
pub struct DimensionFieldInfo {
    pub source_type: String,
    pub source_field: String,
    pub is_singleton: bool,
}

/// Resolve schema dimensions and the variants declared by selected CFD sources.
#[must_use]
pub(crate) fn dimensions_for_project(
    project: &Project,
    schema: &CftSchema,
    model: &crate::CfdDataModel,
) -> Vec<DimensionInfo> {
    let mut out = Vec::new();
    for dimension in schema.all_dimensions() {
        let name = dimension.name.as_str();
        let display_name = project.config().dimensions.get(name).map_or_else(
            || builtin_display_name(name).unwrap_or(name).to_string(),
            |config| resolved_display_name(name, config),
        );
        let info_fields = dimension
            .fields
            .iter()
            .map(|field| DimensionFieldInfo {
                source_type: field.declaring_type.to_string(),
                source_field: field.name.to_string(),
                is_singleton: schema
                    .resolve_type(field.declaring_type.as_str())
                    .is_some_and(|ty| ty.is_singleton()),
            })
            .collect();
        out.push(DimensionInfo {
            name: name.to_string(),
            display_name,
            variants: model
                .dimension_variants(name)
                .map(ToString::to_string)
                .collect(),
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
pub(crate) fn resolved_display_name(name: &str, config: &DimensionConfig) -> String {
    if let Some(custom) = &config.display_name {
        return custom.clone();
    }
    builtin_display_name(name).map_or_else(|| name.to_string(), str::to_string)
}

#[must_use]
pub(crate) const fn builtin_display_name(name: &str) -> Option<&'static str> {
    match name.as_bytes() {
        b"language" => Some("本地化"),
        _ => None,
    }
}
