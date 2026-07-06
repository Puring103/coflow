use crate::dimensions::DimensionField;
use coflow_api::{
    Diagnostic, DiagnosticSet, DimensionSourceEntry, DimensionSourceRequest, Label,
    ProviderRegistry, ResolvedSource, Severity, SourceLocation, SourceLocationSpec, TableContext,
};
use coflow_data_model::{CfdDataModel, CfdValue};
use coflow_project::Project;
use serde_json::json;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[must_use]
pub fn regenerate_dimension_sources(
    project: &Project,
    model: &CfdDataModel,
    fields: &[DimensionField],
    registry: &ProviderRegistry,
) -> DimensionGenerationResult {
    let mut diagnostics = DiagnosticSet::empty();
    let mut transaction = DimensionGenerationTransaction::default();
    for (dimension, config) in &project.config.dimensions {
        let dimension_fields = fields
            .iter()
            .filter(|field| field.dimension == *dimension)
            .collect::<Vec<_>>();
        if dimension_fields.is_empty() {
            continue;
        }
        let Some(out_dir) = config.out_dir.as_ref() else {
            diagnostics.push(dimension_diagnostic(
                &project.config_path,
                dimension,
                "DIM-CONFIG-003",
                format!("dimensions.{dimension}.out_dir is required"),
            ));
            continue;
        };
        let out_dir = project.resolve_path(out_dir);

        for field in dimension_fields {
            let provider_id = if field.is_singleton { "cfd" } else { "csv" };
            let Some(manager) = registry.dimension_source_manager(provider_id) else {
                diagnostics.push(dimension_diagnostic(
                    &project.config_path,
                    dimension,
                    "DIM-SOURCE-002",
                    format!("dimension source provider `{provider_id}` is not registered"),
                ));
                continue;
            };
            let path = dimension_source_path(&out_dir, field);
            transaction.snapshot_file(&path, dimension);
            let source = dimension_resolved_source(project, field, &path, provider_id);
            let entries = dimension_entries(model, field);
            let result = manager.sync_dimension_source(
                TableContext {
                    project_root: &project.root_dir,
                    schema: None,
                },
                &DimensionSourceRequest {
                    source: &source,
                    entries: &entries,
                    variants: &config.variants,
                },
            );
            if let Err(err) = result {
                diagnostics.extend(err);
            }
        }
    }
    DimensionGenerationResult {
        transaction,
        diagnostics,
    }
}

#[derive(Debug, Default)]
pub struct DimensionGenerationResult {
    pub transaction: DimensionGenerationTransaction,
    pub diagnostics: DiagnosticSet,
}

#[derive(Debug, Default)]
pub struct DimensionGenerationTransaction {
    snapshots: BTreeMap<PathBuf, FileSnapshot>,
}

impl DimensionGenerationTransaction {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.snapshots.is_empty()
    }

    pub fn rollback(self, config_path: &Path) -> DiagnosticSet {
        let mut diagnostics = DiagnosticSet::empty();
        for snapshot in self.snapshots.into_values().rev() {
            if let Err(err) = snapshot.restore() {
                diagnostics.push(dimension_diagnostic(
                    config_path,
                    &snapshot.dimension,
                    "DIM-SOURCE-ROLLBACK-001",
                    format!(
                        "failed to roll back dimension source `{}`: {err}",
                        snapshot.path.display()
                    ),
                ));
            }
        }
        diagnostics
    }

    fn snapshot_file(&mut self, path: &Path, dimension: &str) {
        if self.snapshots.contains_key(path) {
            return;
        }
        let original = match fs::read_to_string(path) {
            Ok(text) => Some(text),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
            Err(_) => None,
        };
        self.snapshots.insert(
            path.to_path_buf(),
            FileSnapshot {
                path: path.to_path_buf(),
                dimension: dimension.to_string(),
                original,
            },
        );
    }
}

#[derive(Debug)]
struct FileSnapshot {
    path: PathBuf,
    dimension: String,
    original: Option<String>,
}

impl FileSnapshot {
    fn restore(&self) -> std::io::Result<()> {
        match &self.original {
            Some(text) => fs::write(&self.path, text),
            None => match fs::remove_file(&self.path) {
                Ok(()) => Ok(()),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(err) => Err(err),
            },
        }
    }
}

fn dimension_source_path(out_dir: &Path, field: &DimensionField) -> PathBuf {
    if field.is_singleton {
        out_dir.join(format!("{}.cfd", field.source_type))
    } else {
        out_dir.join(format!("{}_{}.csv", field.bucket, field.source_field))
    }
}

fn dimension_resolved_source(
    project: &Project,
    field: &DimensionField,
    path: &Path,
    provider_id: &str,
) -> ResolvedSource {
    let display_name = path.strip_prefix(&project.root_dir).map_or_else(
        |_| path.display().to_string(),
        coflow_project::path_to_slash,
    );
    ResolvedSource {
        provider_id: provider_id.to_string(),
        location: SourceLocationSpec::Path(path.to_path_buf()),
        options: if provider_id == "csv" {
            json!({
                "sheets": [{
                    "sheet": format!("{}_{}", field.bucket, field.source_field),
                    "type": field.synthesized_type,
                }]
            })
        } else {
            serde_json::Value::Object(serde_json::Map::new())
        },
        display_name,
    }
}

fn dimension_entries(model: &CfdDataModel, field: &DimensionField) -> Vec<DimensionSourceEntry> {
    if field.is_singleton {
        model
            .records_of_type(&field.source_type)
            .next()
            .map(|(_, record)| DimensionSourceEntry {
                key: field.source_field.clone(),
                actual_type: field.synthesized_type.clone(),
                default: record
                    .fields()
                    .get(&field.source_field)
                    .cloned()
                    .unwrap_or(CfdValue::Null),
            })
            .into_iter()
            .collect()
    } else {
        model
            .records_of_type(&field.source_type)
            .map(|(_, record)| DimensionSourceEntry {
                key: record.key().to_string(),
                actual_type: field.synthesized_type.clone(),
                default: record
                    .fields()
                    .get(&field.source_field)
                    .cloned()
                    .unwrap_or(CfdValue::Null),
            })
            .collect()
    }
}

fn dimension_diagnostic(
    config_path: &Path,
    dimension: &str,
    code: &str,
    message: impl Into<String>,
) -> Diagnostic {
    Diagnostic {
        code: code.to_string(),
        stage: "PROJECT".to_string(),
        severity: Severity::Error,
        message: message.into(),
        primary: Some(Label {
            location: SourceLocation::ProjectConfig {
                path: config_path.to_path_buf(),
                key_path: vec!["dimensions".to_string(), dimension.to_string()],
            },
            message: None,
        }),
        related: Vec::new(),
    }
}
