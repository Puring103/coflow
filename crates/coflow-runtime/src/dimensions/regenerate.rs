use crate::dimensions::DimensionField;
use coflow_api::{
    DecodedSourceOptions, Diagnostic, DiagnosticSet, DimensionSourceEntry,
    DimensionSourceOptionsRequest, DimensionSourceRequest, Label, ProviderRegistry, ResolvedSource,
    Severity, SourceLocation, SourceLocationSpec, TableContext,
};
use coflow_cft::CftSchema;
use coflow_data_model::{CfdDataModel, CfdValue};
use coflow_project::Project;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

#[must_use]
pub fn regenerate_dimension_sources(
    project: &Project,
    schema: &CftSchema,
    model: &CfdDataModel,
    fields: &[DimensionField],
    registry: &ProviderRegistry,
) -> DimensionGenerationResult {
    let plan_result = plan_dimension_generation(project, schema, model, fields);
    if !plan_result.diagnostics.is_empty() {
        return DimensionGenerationResult {
            diagnostics: plan_result.diagnostics,
            ..DimensionGenerationResult::default()
        };
    }
    let mut result = commit_dimension_generation(project, plan_result.plan, registry);
    let mut diagnostics = plan_result.diagnostics;
    diagnostics.extend(result.diagnostics);
    result.diagnostics = diagnostics;
    result
}

#[must_use]
pub(crate) fn plan_dimension_generation(
    project: &Project,
    schema: &CftSchema,
    model: &CfdDataModel,
    fields: &[DimensionField],
) -> DimensionGenerationPlanResult {
    let mut diagnostics = validate_dimension_directories(project);
    if !diagnostics.is_empty() {
        return DimensionGenerationPlanResult {
            plan: DimensionGenerationPlan::default(),
            diagnostics,
        };
    }

    let mut operations = Vec::new();
    for (dimension, config) in &project.config.dimensions {
        let result = plan_configured_dimension(project, schema, model, fields, dimension, config);
        operations.extend(result.plan.operations);
        diagnostics.extend(result.diagnostics);
    }

    DimensionGenerationPlanResult {
        plan: DimensionGenerationPlan { operations },
        diagnostics,
    }
}

fn validate_dimension_directories(project: &Project) -> DiagnosticSet {
    let mut diagnostics = DiagnosticSet::empty();
    let owned_dirs = project
        .config
        .dimensions
        .iter()
        .filter_map(|(dimension, config)| {
            config
                .out_dir
                .as_ref()
                .map(|out_dir| (dimension.as_str(), project.resolve_path(out_dir)))
        })
        .collect::<Vec<_>>();
    for (index, (dimension, path)) in owned_dirs.iter().enumerate() {
        for (other_dimension, other_path) in owned_dirs.iter().skip(index + 1) {
            if coflow_project::path_is_same_or_descendant(path, other_path)
                || coflow_project::path_is_same_or_descendant(other_path, path)
            {
                diagnostics.push(dimension_diagnostic(
                    &project.config_path,
                    other_dimension,
                    "DIM-SOURCE-007",
                    format!(
                        "dimensions.{other_dimension}.out_dir overlaps dimensions.{dimension}.out_dir; every dimension requires an exclusive managed directory"
                    ),
                ));
            }
        }
    }
    diagnostics
}

fn plan_configured_dimension(
    project: &Project,
    schema: &CftSchema,
    model: &CfdDataModel,
    fields: &[DimensionField],
    dimension: &str,
    config: &coflow_project::DimensionConfig,
) -> DimensionGenerationPlanResult {
    let mut diagnostics = DiagnosticSet::empty();
    let Some(out_dir) = config.out_dir.as_ref() else {
        diagnostics.push(dimension_diagnostic(
            &project.config_path,
            dimension,
            "DIM-CONFIG-003",
            format!("dimensions.{dimension}.out_dir is required"),
        ));
        return DimensionGenerationPlanResult {
            plan: DimensionGenerationPlan::default(),
            diagnostics,
        };
    };
    let out_dir = project.resolve_path(out_dir);
    let mut expected_paths = BTreeSet::new();
    let mut dimension_operations = BTreeMap::<String, DimensionGenerationOperation>::new();

    for field in fields
        .iter()
        .filter(|field| field.dimension.as_str() == dimension)
    {
        let provider_id = if field.is_singleton { "cfd" } else { "csv" };
        let path = dimension_source_path(&out_dir, field);
        let path_identity = coflow_project::normalized_path_identity(&path);
        expected_paths.insert(path_identity.clone());
        let operation = DimensionGenerationOperation {
            dimension: dimension.to_string(),
            provider_id: provider_id.to_string(),
            path: path.clone(),
            sheet: format!("{}_{}", field.bucket, field.source_field),
            actual_type: field.source_type.to_string(),
            entries: dimension_entries(schema, model, field),
            variants: config.variants.clone(),
            bucket: field.bucket.to_string(),
            is_singleton: field.is_singleton,
        };
        match dimension_operations.entry(path_identity) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(operation);
            }
            std::collections::btree_map::Entry::Occupied(mut entry)
                if field.is_singleton
                    && entry.get().is_singleton
                    && entry.get().actual_type == field.source_type.as_str() =>
            {
                entry.get_mut().entries.extend(operation.entries);
            }
            std::collections::btree_map::Entry::Occupied(entry) => {
                diagnostics.push(dimension_diagnostic(
                    &project.config_path,
                    dimension,
                    "DIM-SOURCE-PATH-CONFLICT",
                    format!(
                        "dimension fields map to the same managed source `{}`",
                        entry.get().path.display()
                    ),
                ));
            }
        }
    }
    let dimension_operations = dimension_operations.into_values().collect::<Vec<_>>();
    let reconciliations = match reconcile_dimension_sources(
        &project.config_path,
        dimension,
        &out_dir,
        &expected_paths,
        &dimension_operations,
    ) {
        Ok(operations) => operations,
        Err(error) => {
            diagnostics.extend(error);
            return DimensionGenerationPlanResult {
                plan: DimensionGenerationPlan::default(),
                diagnostics,
            };
        }
    };
    let operations = reconciliations
        .into_iter()
        .chain(
            dimension_operations
                .into_iter()
                .map(DimensionGenerationPlanOp::Sync),
        )
        .collect();

    DimensionGenerationPlanResult {
        plan: DimensionGenerationPlan { operations },
        diagnostics,
    }
}

fn reconcile_dimension_sources(
    config_path: &Path,
    dimension: &str,
    out_dir: &Path,
    expected_paths: &BTreeSet<String>,
    operations: &[DimensionGenerationOperation],
) -> Result<Vec<DimensionGenerationPlanOp>, DiagnosticSet> {
    let entries = match fs::read_dir(out_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(DiagnosticSet::one(dimension_diagnostic(
                config_path,
                dimension,
                "DIM-SOURCE-DISCOVERY-001",
                format!(
                    "failed to read dimension source directory `{}`: {error}",
                    out_dir.display()
                ),
            )));
        }
    };
    let mut paths = entries
        .map(|entry| {
            entry.map(|entry| entry.path()).map_err(|error| {
                DiagnosticSet::one(dimension_diagnostic(
                    config_path,
                    dimension,
                    "DIM-SOURCE-DISCOVERY-001",
                    format!(
                        "failed to enumerate dimension source directory `{}`: {error}",
                        out_dir.display()
                    ),
                ))
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    paths.sort();
    let stale_paths = paths
        .into_iter()
        .filter(|path| {
            path.extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| matches!(extension, "csv" | "cfd"))
                && !expected_paths.contains(&coflow_project::normalized_path_identity(path))
        })
        .collect::<Vec<_>>();

    let mut reconciliations = Vec::new();
    for stale_path in &stale_paths {
        let mut candidates = Vec::new();
        for operation in operations {
            let target_exists = operation.path.try_exists().map_err(|error| {
                DiagnosticSet::one(dimension_diagnostic(
                    config_path,
                    dimension,
                    "DIM-SOURCE-DISCOVERY-001",
                    format!(
                        "failed to inspect dimension source `{}`: {error}",
                        operation.path.display()
                    ),
                ))
            })?;
            if !target_exists && operation.matches_renamed_source(stale_path) {
                candidates.push(operation);
            }
        }
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
    Ok(reconciliations)
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
        match operation {
            DimensionGenerationPlanOp::Move { from, to } => {
                commit_dimension_move(
                    &mut transaction,
                    &project.config_path,
                    from,
                    to,
                    &mut diagnostics,
                    &mut changed_paths,
                );
            }
            DimensionGenerationPlanOp::Remove(path) => {
                commit_dimension_remove(
                    &mut transaction,
                    &project.config_path,
                    path,
                    &mut diagnostics,
                    &mut changed_paths,
                );
            }
            DimensionGenerationPlanOp::Sync(operation) => commit_dimension_sync(
                project,
                registry,
                &mut transaction,
                operation,
                &mut diagnostics,
                &mut changed_paths,
            ),
        }
    }

    DimensionGenerationResult {
        transaction,
        diagnostics,
        changed_paths: changed_paths.into_iter().collect(),
    }
}

fn commit_dimension_move(
    transaction: &mut DimensionGenerationTransaction,
    config_path: &Path,
    from: PathBuf,
    to: PathBuf,
    diagnostics: &mut DiagnosticSet,
    changed_paths: &mut BTreeSet<PathBuf>,
) {
    if let Err(error) = transaction.move_file(&from, &to, config_path) {
        diagnostics.extend(error);
        return;
    }
    if let Err(error) = fs::rename(&from, &to) {
        diagnostics.push(Diagnostic::error(
            "DIM-SOURCE-005",
            "PROJECT",
            format!(
                "failed to migrate dimension source `{}` to `{}`: {error}",
                from.display(),
                to.display()
            ),
        ));
    } else {
        changed_paths.insert(from);
        changed_paths.insert(to);
    }
}

fn commit_dimension_remove(
    transaction: &mut DimensionGenerationTransaction,
    config_path: &Path,
    path: PathBuf,
    diagnostics: &mut DiagnosticSet,
    changed_paths: &mut BTreeSet<PathBuf>,
) {
    if let Err(error) = transaction.remove_file(&path, config_path) {
        diagnostics.extend(error);
        return;
    }
    if let Err(error) = fs::remove_file(&path) {
        diagnostics.push(Diagnostic::error(
            "DIM-SOURCE-006",
            "PROJECT",
            format!(
                "failed to remove obsolete dimension source `{}`: {error}",
                path.display()
            ),
        ));
    } else {
        changed_paths.insert(path);
    }
}

fn commit_dimension_sync(
    project: &Project,
    registry: &ProviderRegistry,
    transaction: &mut DimensionGenerationTransaction,
    operation: DimensionGenerationOperation,
    diagnostics: &mut DiagnosticSet,
    changed_paths: &mut BTreeSet<PathBuf>,
) {
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
        return;
    };
    let options = match manager.source_options(&DimensionSourceOptionsRequest {
        sheet: &operation.sheet,
        actual_type: &operation.actual_type,
    }) {
        Ok(options) => options,
        Err(error) => {
            diagnostics.extend(error);
            return;
        }
    };
    let source =
        dimension_resolved_source(project, &operation.path, &operation.provider_id, options);
    if let Err(error) =
        transaction.snapshot_file(&operation.path, &operation.dimension, &project.config_path)
    {
        diagnostics.extend(error);
        return;
    }
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
        Ok(result) if result.changed => {
            changed_paths.insert(operation.path);
        }
        Ok(_) => {}
        Err(error) => diagnostics.extend(error),
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

    fn snapshot_file(
        &mut self,
        path: &Path,
        dimension: &str,
        config_path: &Path,
    ) -> Result<(), DiagnosticSet> {
        if self.snapshots.contains_key(path) {
            return Ok(());
        }
        let original = match fs::read(path) {
            Ok(bytes) => Some(bytes),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
            Err(err) => {
                return Err(DiagnosticSet::one(dimension_diagnostic(
                    config_path,
                    dimension,
                    "DIM-SOURCE-SNAPSHOT-001",
                    format!(
                        "failed to snapshot dimension source `{}` before generation: {err}",
                        path.display()
                    ),
                )));
            }
        };
        self.snapshots.insert(
            path.to_path_buf(),
            FileSnapshot {
                path: path.to_path_buf(),
                dimension: dimension.to_string(),
                original,
            },
        );
        Ok(())
    }

    fn move_file(
        &mut self,
        from: &Path,
        to: &Path,
        config_path: &Path,
    ) -> Result<(), DiagnosticSet> {
        self.snapshot_file(from, "generated", config_path)?;
        self.snapshot_file(to, "generated", config_path)
    }

    fn remove_file(&mut self, path: &Path, config_path: &Path) -> Result<(), DiagnosticSet> {
        self.snapshot_file(path, "generated", config_path)
    }
}

#[derive(Debug)]
struct FileSnapshot {
    path: PathBuf,
    dimension: String,
    original: Option<Vec<u8>>,
}

impl FileSnapshot {
    fn restore(&self) -> std::io::Result<()> {
        self.original.as_ref().map_or_else(
            || match fs::remove_file(&self.path) {
                Ok(()) => Ok(()),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(err) => Err(err),
            },
            |bytes| fs::write(&self.path, bytes),
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

fn dimension_entries(
    schema: &CftSchema,
    model: &CfdDataModel,
    field: &DimensionField,
) -> Vec<DimensionSourceEntry> {
    if field.is_singleton {
        model
            .records_assignable_to(schema, &field.source_type)
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
            .records_assignable_to(schema, &field.source_type)
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

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::{
        commit_dimension_generation, DimensionGenerationOperation, DimensionGenerationPlan,
        DimensionGenerationPlanOp, DimensionGenerationTransaction,
    };
    use coflow_api::ProviderRegistry;
    use coflow_project::Project;

    fn test_project(root: &std::path::Path) -> Project {
        std::fs::write(root.join("schema.cft"), "type Item { name: string; }")
            .expect("write schema");
        std::fs::write(
            root.join("coflow.yaml"),
            "schema: schema.cft\nsources: []\n",
        )
        .expect("write config");
        Project::open_schema_only(Some(root)).expect("open project")
    }

    #[test]
    fn snapshot_errors_are_reported_and_do_not_enlist_the_path() {
        let root = std::env::temp_dir().join(format!(
            "coflow-runtime-dimension-snapshot-error-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let source = root.join("Item_name.csv");
        std::fs::create_dir_all(&source).expect("create directory at source path");
        let config = root.join("coflow.yaml");
        let mut transaction = DimensionGenerationTransaction::default();

        let diagnostics = transaction
            .snapshot_file(&source, "language", &config)
            .expect_err("directories cannot be snapshotted as generated files");

        assert!(diagnostics
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "DIM-SOURCE-SNAPSHOT-001"));
        assert!(transaction.is_empty());
        std::fs::remove_dir_all(root).expect("remove temp dir");
    }

    #[test]
    fn generation_operation_failures_report_stable_codes() {
        let root = std::env::temp_dir().join(format!(
            "coflow-runtime-dimension-operation-errors-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("create temp dir");
        let project = test_project(&root);
        let missing_move = root.join("missing-old.csv");
        let missing_remove = root.join("missing-stale.csv");
        let generated = root.join("generated.csv");
        let plan = DimensionGenerationPlan {
            operations: vec![
                DimensionGenerationPlanOp::Move {
                    from: missing_move,
                    to: root.join("moved.csv"),
                },
                DimensionGenerationPlanOp::Remove(missing_remove),
                DimensionGenerationPlanOp::Sync(DimensionGenerationOperation {
                    dimension: "language".to_string(),
                    provider_id: "missing-provider".to_string(),
                    path: generated,
                    sheet: "Item_name".to_string(),
                    actual_type: "Item".to_string(),
                    entries: Vec::new(),
                    variants: vec!["zh".to_string()],
                    bucket: "Item".to_string(),
                    is_singleton: false,
                }),
            ],
        };

        let result = commit_dimension_generation(&project, plan, &ProviderRegistry::default());
        let codes = result
            .diagnostics
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        assert!(codes.contains("DIM-SOURCE-002"));
        assert!(codes.contains("DIM-SOURCE-005"));
        assert!(codes.contains("DIM-SOURCE-006"));
        std::fs::remove_dir_all(root).expect("remove temp dir");
    }

    #[test]
    fn rollback_reports_restore_failures() {
        let root = std::env::temp_dir().join(format!(
            "coflow-runtime-dimension-rollback-error-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("create temp dir");
        let source = root.join("Item_name.csv");
        let config = root.join("coflow.yaml");
        std::fs::write(&source, "original").expect("write source");
        let mut transaction = DimensionGenerationTransaction::default();
        transaction
            .snapshot_file(&source, "language", &config)
            .expect("snapshot source");
        std::fs::remove_file(&source).expect("remove source");
        std::fs::create_dir(&source).expect("replace source with directory");

        let diagnostics = transaction.rollback(&config);

        assert!(diagnostics
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "DIM-SOURCE-ROLLBACK-001"));
        std::fs::remove_dir_all(root).expect("remove temp dir");
    }
}
