use super::{project_diagnostic, ConfiguredSource, ResolvedLoaderSource, SourceResolver};
use crate::dimensions::{DimensionField, DimensionRuntimePlan};
use coflow_api::{Diagnostic, DiagnosticSet, SourceLocationSpec};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub(super) fn resolve_dimension_sources(
    resolver: &SourceResolver<'_>,
    plan: &DimensionRuntimePlan,
) -> Result<Vec<(ResolvedLoaderSource, DimensionField)>, DiagnosticSet> {
    let mut sources = Vec::new();
    let mut diagnostics = DiagnosticSet::empty();
    for (dimension, config) in &resolver.project.config.dimensions {
        let Some(out_dir) = config.out_dir.as_ref() else {
            continue;
        };
        let dimension_fields = plan.fields_for(dimension).collect::<Vec<_>>();
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
            let (configured, matched_fields) =
                match configured_dimension_source(resolver, &directory, &dimension_fields, path) {
                    Ok(Some(source)) => source,
                    Ok(None) => continue,
                    Err(error) => {
                        diagnostics.extend(error);
                        continue;
                    }
                };
            match resolver.resolve_implicit(&configured) {
                Ok(resolved_sources) => {
                    for resolved in resolved_sources {
                        for field in &matched_fields {
                            sources.push((
                                (Arc::clone(&resolved.0), resolved.1.clone()),
                                field.clone(),
                            ));
                        }
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
) -> Result<Option<(ConfiguredSource, Vec<DimensionField>)>, DiagnosticSet> {
    let Some(extension) = path.extension().and_then(|ext| ext.to_str()) else {
        return Ok(None);
    };
    let extension = extension.to_string();
    if !matches!(extension.as_str(), "csv" | "cfd") {
        return Ok(None);
    }
    let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
        return Ok(None);
    };
    let matched_fields = fields_for_file_stem(fields, stem, &extension);
    if matched_fields.is_empty() {
        return Ok(None);
    }
    let singleton_group = matched_fields.iter().all(|field| field.is_singleton)
        && matched_fields
            .iter()
            .all(|field| field.source_type == matched_fields[0].source_type);
    if matched_fields.len() > 1 && !singleton_group {
        return Err(DiagnosticSet::one(Diagnostic::error(
            "DIM-SOURCE-PATH-CONFLICT",
            "PROJECT",
            format!(
                "multiple dimension fields map to managed source `{}`",
                path.display()
            ),
        )));
    }
    let display_name = path.strip_prefix(&resolver.project.root_dir).map_or_else(
        |_| path.display().to_string(),
        coflow_project::path_to_slash,
    );
    Ok(Some((
        ConfiguredSource {
            provider_id: String::new(),
            location: SourceLocationSpec::new(path),
            options: source_options(matched_fields[0], &extension),
            display_name: if display_name.is_empty() {
                directory.display().to_string()
            } else {
                display_name
            },
            source_index: None,
        },
        matched_fields.into_iter().cloned().collect(),
    )))
}

fn fields_for_file_stem<'a>(
    fields: &'a [&DimensionField],
    stem: &str,
    extension: &str,
) -> Vec<&'a DimensionField> {
    fields
        .iter()
        .copied()
        .filter(|field| {
            if extension == "cfd" && field.is_singleton {
                stem == field.source_type.as_str()
            } else {
                stem == format!("{}_{}", field.bucket, field.source_field)
            }
        })
        .collect()
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
