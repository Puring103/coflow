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
            .filter(|field| field.dimension.as_str() == dimension)
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
        let mut dimension_operations = Vec::new();

        for field in dimension_fields {
            let provider_id = if field.is_singleton { "cfd" } else { "csv" };
            let path = dimension_source_path(&out_dir, field);
            expected_paths.insert(path.clone());
            dimension_operations.push(DimensionGenerationOperation {
                dimension: dimension.clone(),
                provider_id: provider_id.to_string(),
                path: path.clone(),
                sheet: format!("{}_{}", field.bucket, field.source_field),
                actual_type: field.source_type.to_string(),
                entries: dimension_entries(model, field),
                variants: config.variants.clone(),
                bucket: field.bucket.to_string(),
                is_singleton: field.is_singleton,
            });
        }
        operations.extend(reconcile_dimension_sources(
            &out_dir,
            &expected_paths,
            &dimension_operations,
        ));
        operations.extend(
            dimension_operations
                .into_iter()
                .map(DimensionGenerationPlanOp::Sync),
        );
    }

    DimensionGenerationPlanResult {
        plan: DimensionGenerationPlan { operations },
        diagnostics,
    }
}

fn reconcile_dimension_sources(
    out_dir: &Path,
    expected_paths: &BTreeSet<PathBuf>,
    operations: &[DimensionGenerationOperation],
) -> Vec<DimensionGenerationPlanOp> {
    let Ok(entries) = fs::read_dir(out_dir) else {
        return Vec::new();
    };
    let mut stale_paths = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| matches!(extension, "csv" | "cfd"))
                && !expected_paths.contains(path)
        })
        .collect::<Vec<_>>();
    stale_paths.sort();

    let mut reconciliations = Vec::new();
    for stale_path in &stale_paths {
        let candidates = operations
            .iter()
            .filter(|operation| {
                !operation.path.exists() && operation.matches_renamed_source(stale_path)
            })
            .collect::<Vec<_>>();
        if candidates.len() == 1 {
            reconciliations.push(DimensionGenerationPlanOp::Move {
                from: stale_path.clone(),
                to: candidates[0].path.clone(),
            });
        }
    }

    let migrated = reconciliations
        .iter()
        .filter_map(|operation| match operation {
            DimensionGenerationPlanOp::Move { from, .. } => Some(from.clone()),
            DimensionGenerationPlanOp::Sync(_) | DimensionGenerationPlanOp::Remove(_) => None,
        })
        .collect::<BTreeSet<_>>();
    reconciliations.extend(
        stale_paths
            .into_iter()
            .filter(|path| !migrated.contains(path))
            .map(DimensionGenerationPlanOp::Remove),
    );
    reconciliations
}

#[must_use]
pub(crate) fn commit_dimension_generation(
    project: &Project,
    plan: DimensionGenerationPlan,
    registry: &ProviderRegistry,
) -> DimensionGenerationResult {
    let mut diagnostics = DiagnosticSet::empty();
    let mut transaction = DimensionGenerationTransaction::default();
    let mut changed_paths = BTreeSet::new();

    for operation in plan.operations {
        let operation = match operation {
            DimensionGenerationPlanOp::Move { from, to } => {
                transaction.move_file(&from, &to);
                if let Err(err) = fs::rename(&from, &to) {
                    diagnostics.push(Diagnostic::error(
                        "DIM-SOURCE-005",
                        "PROJECT",
                        format!(
                            "failed to migrate dimension source `{}` to `{}`: {err}",
                            from.display(),
                            to.display()
                        ),
                    ));
                } else {
                    changed_paths.insert(from);
                    changed_paths.insert(to);
                }
                continue;
            }
            DimensionGenerationPlanOp::Remove(path) => {
                transaction.remove_file(&path);
                if let Err(err) = fs::remove_file(&path) {
                    diagnostics.push(Diagnostic::error(
                        "DIM-SOURCE-006",
                        "PROJECT",
                        format!(
                            "failed to remove obsolete dimension source `{}`: {err}",
                            path.display()
                        ),
                    ));
                } else {
                    changed_paths.insert(path);
                }
                continue;
            }
            DimensionGenerationPlanOp::Sync(operation) => operation,
        };
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
        match result {
            Ok(result) => {
                if result.changed {
                    changed_paths.insert(operation.path);
                }
            }
            Err(err) => diagnostics.extend(err),
        }
    }

    DimensionGenerationResult {
        transaction,
        diagnostics,
        changed_paths: changed_paths.into_iter().collect(),
    }
}

#[derive(Debug, Default)]
pub(crate) struct DimensionGenerationPlanResult {
    pub plan: DimensionGenerationPlan,
    pub diagnostics: DiagnosticSet,
}

#[derive(Debug, Default)]
pub(crate) struct DimensionGenerationPlan {
    operations: Vec<DimensionGenerationPlanOp>,
}

#[derive(Debug)]
enum DimensionGenerationPlanOp {
    Move { from: PathBuf, to: PathBuf },
    Remove(PathBuf),
    Sync(DimensionGenerationOperation),
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
    bucket: String,
    is_singleton: bool,
}

impl DimensionGenerationOperation {
    fn matches_renamed_source(&self, path: &Path) -> bool {
        !self.is_singleton
            && path.extension().and_then(|extension| extension.to_str()) == Some("csv")
            && path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .is_some_and(|stem| stem.starts_with(&format!("{}_", self.bucket)))
    }
}

#[derive(Debug, Default)]
pub struct DimensionGenerationResult {
    pub transaction: DimensionGenerationTransaction,
    pub diagnostics: DiagnosticSet,
    pub changed_paths: Vec<PathBuf>,
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

    fn move_file(&mut self, from: &Path, to: &Path) {
        self.snapshot_file(from, "generated");
        self.snapshot_file(to, "generated");
    }

    fn remove_file(&mut self, path: &Path) {
        self.snapshot_file(path, "generated");
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
                key: field.source_field.to_string(),
                actual_type: field.source_type.to_string(),
                default: record
                    .fields()
                    .get(field.source_field.as_str())
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
                actual_type: field.source_type.to_string(),
                default: record
                    .fields()
                    .get(field.source_field.as_str())
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
