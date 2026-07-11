use crate::dimensions::DimensionField;
use coflow_api::{
    DecodedSourceOptions, Diagnostic, DiagnosticSet, DimensionSourceEntry,
    DimensionSourceOptionsRequest, DimensionSourceRequest, Label, ProviderRegistry, ResolvedSource,
    Severity, SourceLocation, SourceLocationSpec, TableContext,
};
use coflow_data_model::{CfdDataModel, CfdValue};
use coflow_project::Project;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

#[must_use]
pub fn regenerate_dimension_sources(
    project: &Project,
    model: &CfdDataModel,
    fields: &[DimensionField],
    registry: &ProviderRegistry,
) -> DimensionGenerationResult {
    let plan_result = plan_dimension_generation(project, model, fields);
    let mut result = commit_dimension_generation(project, plan_result.plan, registry);
    let mut diagnostics = plan_result.diagnostics;
    diagnostics.extend(result.diagnostics);
    result.diagnostics = diagnostics;
    result
}

#[must_use]
pub(crate) fn plan_dimension_generation(
    project: &Project,
    model: &CfdDataModel,
    fields: &[DimensionField],
) -> DimensionGenerationPlanResult {
    let mut diagnostics = DiagnosticSet::empty();
    let mut operations = Vec::new();
    for (dimension, config) in &project.config.dimensions {
        let dimension_fields = fields
            .iter()
            .filter(|field| field.dimension == *dimension)
            .collect::<Vec<_>>();
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
        let mut expected_paths = BTreeSet::new();

        for field in dimension_fields {
            let provider_id = if field.is_singleton { "cfd" } else { "csv" };
            let path = dimension_source_path(&out_dir, field);
            expected_paths.insert(path.clone());
            operations.push(DimensionGenerationOperation {
                dimension: dimension.clone(),
                provider_id: provider_id.to_string(),
                path: path.clone(),
                sheet: format!("{}_{}", field.bucket, field.source_field),
                actual_type: field.synthesized_type.clone(),
                entries: dimension_entries(model, field),
                variants: config.variants.clone(),
            });
        }
        diagnostics.extend(unmanaged_dimension_source_diagnostics(
            &project.config_path,
            dimension,
            &out_dir,
            &expected_paths,
        ));
    }

    DimensionGenerationPlanResult {
        plan: DimensionGenerationPlan { operations },
        diagnostics,
    }
}

fn unmanaged_dimension_source_diagnostics(
    config_path: &Path,
    dimension: &str,
    out_dir: &Path,
    expected_paths: &BTreeSet<PathBuf>,
) -> DiagnosticSet {
    let mut diagnostics = DiagnosticSet::empty();
    let Ok(entries) = fs::read_dir(out_dir) else {
        return diagnostics;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let is_dimension_file = path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| matches!(extension, "csv" | "cfd"));
        if is_dimension_file && !expected_paths.contains(&path) {
            diagnostics.push(dimension_diagnostic(
                config_path,
                dimension,
                "DIM-SOURCE-004",
                format!(
                    "dimension source `{}` is no longer managed by the current schema; remove it from dimensions.{dimension}.out_dir",
                    path.display()
                ),
            ));
        }
    }
    diagnostics
}

#[must_use]
pub(crate) fn commit_dimension_generation(
    project: &Project,
    plan: DimensionGenerationPlan,
    registry: &ProviderRegistry,
) -> DimensionGenerationResult {
    let mut diagnostics = DiagnosticSet::empty();
    let mut transaction = DimensionGenerationTransaction::default();

    for operation in plan.operations {
        let Some(manager) = registry.dimension_source_manager(&operation.provider_id) else {
            diagnostics.push(dimension_diagnostic(
                &project.config_path,
                &operation.dimension,
                "DIM-SOURCE-002",
                format!(
                    "dimension source provider `{}` is not registered",
                    operation.provider_id
                ),
            ));
            continue;
        };

        let options = match manager.source_options(&DimensionSourceOptionsRequest {
            sheet: &operation.sheet,
            actual_type: &operation.actual_type,
        }) {
            Ok(options) => options,
            Err(err) => {
                diagnostics.extend(err);
                continue;
            }
        };
        let source =
            dimension_resolved_source(project, &operation.path, &operation.provider_id, options);
        transaction.snapshot_file(&operation.path, &operation.dimension);
        let result = manager.sync_dimension_source(
            TableContext {
                project_root: &project.root_dir,
            },
            &DimensionSourceRequest {
                source: &source,
                entries: &operation.entries,
                variants: &operation.variants,
            },
        );
        if let Err(err) = result {
            diagnostics.extend(err);
        }
    }

    DimensionGenerationResult {
        transaction,
        diagnostics,
    }
}

#[derive(Debug, Default)]
pub(crate) struct DimensionGenerationPlanResult {
    pub plan: DimensionGenerationPlan,
    pub diagnostics: DiagnosticSet,
}

#[derive(Debug, Default)]
pub(crate) struct DimensionGenerationPlan {
    operations: Vec<DimensionGenerationOperation>,
}

#[derive(Debug)]
struct DimensionGenerationOperation {
    dimension: String,
    provider_id: String,
    path: PathBuf,
    sheet: String,
    actual_type: String,
    entries: Vec<DimensionSourceEntry>,
    variants: Vec<String>,
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
        self.original.as_ref().map_or_else(
            || match fs::remove_file(&self.path) {
                Ok(()) => Ok(()),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(err) => Err(err),
            },
            |text| fs::write(&self.path, text),
        )
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
    path: &Path,
    provider_id: &str,
    options: DecodedSourceOptions,
) -> ResolvedSource {
    let display_name = path.strip_prefix(&project.root_dir).map_or_else(
        |_| path.display().to_string(),
        coflow_project::path_to_slash,
    );
    ResolvedSource {
        provider_id: provider_id.to_string(),
        location: SourceLocationSpec::Path(path.to_path_buf()),
        options,
        display_name,
    }
}

fn dimension_entries(model: &CfdDataModel, field: &DimensionField) -> Vec<DimensionSourceEntry> {
    if field.is_singleton {
        model
            .records_assignable_to(&field.source_type)
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
            .records_assignable_to(&field.source_type)
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
