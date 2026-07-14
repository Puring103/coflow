use crate::source_resolution::ConfiguredSource;
use coflow_api::SourceLocationSpec;
use coflow_cft::CftSchema;
use coflow_project::Project;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DimensionField {
    pub dimension: String,
    pub source_type: String,
    pub source_field: String,
    pub bucket: String,
    pub synthesized_type: String,
    pub is_singleton: bool,
}

pub(crate) fn dimension_sources(
    project: &Project,
    fields: &[DimensionField],
) -> Vec<ConfiguredSource> {
    let mut sources = Vec::new();
    for (dimension, config) in &project.config.dimensions {
        let Some(out_dir) = config.out_dir.as_ref() else {
            continue;
        };
        let fields = fields
            .iter()
            .filter(|field| field.dimension == *dimension)
            .collect::<Vec<_>>();
        if fields.is_empty() {
            continue;
        }
        let dir = project.resolve_path(out_dir);
        if !dir.exists() {
            continue;
        }
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        let mut entries = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .collect::<Vec<_>>();
        entries.sort();
        sources.extend(
            entries
                .into_iter()
                .filter_map(|path| source_for_dimension_file(project, &dir, &fields, path)),
        );
    }
    sources
}

pub fn dimension_fields(schema: &CftSchema) -> Vec<DimensionField> {
    let mut fields = Vec::new();
    for schema_type in schema.type_metas() {
        for field in &schema_type.own_fields {
            let Some(dimension) = field.dimension.as_ref() else {
                continue;
            };
            fields.push(DimensionField {
                dimension: dimension.dimension.clone(),
                source_type: schema_type.name.clone(),
                source_field: field.name.clone(),
                bucket: dimension
                    .bucket
                    .clone()
                    .unwrap_or_else(|| schema_type.name.clone()),
                // Missing dimension config is diagnosed before data loading. The
                // fallback only preserves enough identity for that diagnostic.
                synthesized_type: schema
                    .dimension_storage_type(
                        &dimension.dimension,
                        &schema_type.name,
                        &field.name,
                    )
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("{}_{}Variants", schema_type.name, field.name)),
                is_singleton: schema_type.is_singleton,
            });
        }
    }
    fields
}

fn source_for_dimension_file(
    project: &Project,
    dir: &Path,
    fields: &[&DimensionField],
    path: PathBuf,
) -> Option<ConfiguredSource> {
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_string)?;
    let stem = path.file_stem().and_then(|stem| stem.to_str())?;
    let field = field_for_file_stem(fields, stem, &extension)?;
    let provider_id = match extension.as_str() {
        "csv" => "csv",
        "cfd" => "cfd",
        _ => return None,
    };
    let display_name = path.strip_prefix(&project.root_dir).map_or_else(
        |_| path.display().to_string(),
        coflow_project::path_to_slash,
    );
    Some(ConfiguredSource {
        provider_id: provider_id.to_string(),
        location: SourceLocationSpec::Path(path),
        options: source_options(field, &extension),
        display_name: if display_name.is_empty() {
            dir.display().to_string()
        } else {
            display_name
        },
        source_index: None,
    })
}

fn field_for_file_stem<'a>(
    fields: &'a [&DimensionField],
    stem: &str,
    extension: &str,
) -> Option<&'a DimensionField> {
    fields.iter().copied().find(|field| {
        if extension == "cfd" && field.is_singleton {
            stem == field.source_type
        } else {
            stem == format!("{}_{}", field.bucket, field.source_field)
        }
    })
}

fn source_options(field: &DimensionField, extension: &str) -> Value {
    if extension == "csv" {
        json!({
            "sheets": [{
                "sheet": format!("{}_{}", field.bucket, field.source_field),
                "type": field.synthesized_type,
            }]
        })
    } else {
        Value::Object(serde_json::Map::new())
    }
}
