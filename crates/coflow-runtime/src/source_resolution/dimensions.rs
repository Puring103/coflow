use super::{project_diagnostic, ConfiguredSource, ResolvedLoaderSource, SourceResolver};
use crate::dimensions::DimensionField;
use coflow_api::{DiagnosticSet, SourceLocationSpec};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

pub(super) fn resolve_dimension_sources(
    resolver: &SourceResolver<'_>,
    fields: &[DimensionField],
) -> Result<Vec<(ResolvedLoaderSource, DimensionField)>, DiagnosticSet> {
    let mut sources = Vec::new();
    let mut diagnostics = DiagnosticSet::empty();
    for (dimension, config) in &resolver.project.config.dimensions {
        let Some(out_dir) = config.out_dir.as_ref() else {
            continue;
        };
        let dimension_fields = fields
            .iter()
            .filter(|field| field.dimension.as_str() == dimension)
            .collect::<Vec<_>>();
        if dimension_fields.is_empty() {
            continue;
        }
        let directory = resolver.project.resolve_path(out_dir);
        match directory.try_exists() {
            Ok(true) => {}
            Ok(false) => continue,
            Err(error) => {
                diagnostics.extend(discovery_diagnostic(
                    resolver, &directory, "inspect", &error,
                ));
                continue;
            }
        }
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) => {
                diagnostics.extend(discovery_diagnostic(resolver, &directory, "read", &error));
                continue;
            }
        };
        let paths = entries
            .map(|entry| {
                entry.map(|entry| entry.path()).map_err(|error| {
                    discovery_diagnostic(resolver, &directory, "enumerate", &error)
                })
            })
            .collect::<Result<Vec<_>, _>>();
        let mut paths = match paths {
            Ok(paths) => paths,
            Err(error) => {
                diagnostics.extend(error);
                continue;
            }
        };
        paths.sort();

        for path in paths {
            let Some((configured, field)) =
                configured_dimension_source(resolver, &directory, &dimension_fields, path)
            else {
                continue;
            };
            match resolver.resolve_implicit(&configured) {
                Ok(resolved_sources) => {
                    for resolved in resolved_sources {
                        sources.push((resolved, field.clone()));
                    }
                }
                Err(error) => diagnostics.extend(error),
            }
        }
    }
    if diagnostics.is_empty() {
        Ok(sources)
    } else {
        Err(diagnostics)
    }
}

fn discovery_diagnostic(
    resolver: &SourceResolver<'_>,
    directory: &Path,
    operation: &str,
    error: &std::io::Error,
) -> DiagnosticSet {
    DiagnosticSet::one(project_diagnostic(
        &resolver.project.config_path,
        format!(
            "failed to {operation} dimension source directory `{}`: {error}",
            directory.display()
        ),
    ))
}

fn configured_dimension_source(
    resolver: &SourceResolver<'_>,
    directory: &Path,
    fields: &[&DimensionField],
    path: PathBuf,
) -> Option<(ConfiguredSource, DimensionField)> {
    let extension = path.extension().and_then(|ext| ext.to_str())?.to_string();
    if !matches!(extension.as_str(), "csv" | "cfd") {
        return None;
    }
    let stem = path.file_stem().and_then(|stem| stem.to_str())?;
    let field = field_for_file_stem(fields, stem, &extension)?;
    let display_name = path.strip_prefix(&resolver.project.root_dir).map_or_else(
        |_| path.display().to_string(),
        coflow_project::path_to_slash,
    );
    Some((
        ConfiguredSource {
            provider_id: String::new(),
            location: SourceLocationSpec::Path(path),
            options: source_options(field, &extension),
            display_name: if display_name.is_empty() {
                directory.display().to_string()
            } else {
                display_name
            },
            source_index: None,
        },
        field.clone(),
    ))
}

fn field_for_file_stem<'a>(
    fields: &'a [&DimensionField],
    stem: &str,
    extension: &str,
) -> Option<&'a DimensionField> {
    fields.iter().copied().find(|field| {
        if extension == "cfd" && field.is_singleton {
            stem == field.source_type.as_str()
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
                "type": field.source_type.as_str(),
            }]
        })
    } else {
        Value::Object(serde_json::Map::new())
    }
}
