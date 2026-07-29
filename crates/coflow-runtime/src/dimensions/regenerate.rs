mod plan;

use crate::dimensions::DimensionField;
use coflow_api::{
    DecodedSourceOptions, Diagnostic, DiagnosticSet, DimensionSourceEntry,
    DimensionSourceOptionsRequest, DimensionSourceRequest, Label, ProviderRegistry, ResolvedSource,
    Severity, SourceLocation, SourceLocationSpec, TableContext,
};
use coflow_cft::CftSchema;
use coflow_data_model::CfdDataModel;
use coflow_project::Project;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use plan::plan_dimension_generation_scoped;

#[must_use]
pub(crate) fn regenerate_dimension_sources_scoped(
    project: &Project,
    schema: &CftSchema,
    model: &CfdDataModel,
    fields: &[DimensionField],
    affected_fields: Option<&BTreeSet<usize>>,
    registry: &ProviderRegistry,
) -> DimensionGenerationResult {
    let plan_result =
        plan_dimension_generation_scoped(project, schema, model, fields, affected_fields);
    let planned_sources = plan_result.plan.operations.len();
    if !plan_result.diagnostics.is_empty() {
        return DimensionGenerationResult {
            diagnostics: plan_result.diagnostics,
            planned_sources,
            ..DimensionGenerationResult::default()
        };
    }
    let mut result = commit_dimension_generation(project, plan_result.plan, registry);
    let mut diagnostics = plan_result.diagnostics;
    diagnostics.extend(result.diagnostics);
    result.diagnostics = diagnostics;
    result.planned_sources = planned_sources;
    result
}

fn commit_dimension_generation(
    project: &Project,
    plan: DimensionGenerationPlan,
    registry: &ProviderRegistry,
) -> DimensionGenerationResult {
    let mut diagnostics = DiagnosticSet::empty();
    let mut transaction = DimensionGenerationTransaction::default();
    let mut changed_paths = BTreeSet::new();
    let planned_sources = plan.operations.len();
    let mut written_sources = 0_usize;

    for operation in plan.operations {
        let changed = match operation {
            DimensionGenerationPlanOp::Move { from, to } => commit_dimension_move(
                &mut transaction,
                &project.config_path,
                from,
                to,
                &mut diagnostics,
                &mut changed_paths,
            ),
            DimensionGenerationPlanOp::Remove(path) => commit_dimension_remove(
                &mut transaction,
                &project.config_path,
                path,
                &mut diagnostics,
                &mut changed_paths,
            ),
            DimensionGenerationPlanOp::Sync(operation) => commit_dimension_sync(
                project,
                registry,
                &mut transaction,
                operation,
                &mut diagnostics,
                &mut changed_paths,
            ),
        };
        if changed {
            written_sources = written_sources.saturating_add(1);
        }
    }

    DimensionGenerationResult {
        transaction,
        diagnostics,
        changed_paths: changed_paths.into_iter().collect(),
        planned_sources,
        written_sources,
    }
}

fn commit_dimension_move(
    transaction: &mut DimensionGenerationTransaction,
    config_path: &Path,
    from: PathBuf,
    to: PathBuf,
    diagnostics: &mut DiagnosticSet,
    changed_paths: &mut BTreeSet<PathBuf>,
) -> bool {
    if let Err(error) = transaction.move_file(&from, &to, config_path) {
        diagnostics.extend(error);
        return false;
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
        return true;
    }
    false
}

fn commit_dimension_remove(
    transaction: &mut DimensionGenerationTransaction,
    config_path: &Path,
    path: PathBuf,
    diagnostics: &mut DiagnosticSet,
    changed_paths: &mut BTreeSet<PathBuf>,
) -> bool {
    if let Err(error) = transaction.remove_file(&path, config_path) {
        diagnostics.extend(error);
        return false;
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
        return true;
    }
    false
}

fn commit_dimension_sync(
    project: &Project,
    registry: &ProviderRegistry,
    transaction: &mut DimensionGenerationTransaction,
    operation: DimensionGenerationOperation,
    diagnostics: &mut DiagnosticSet,
    changed_paths: &mut BTreeSet<PathBuf>,
) -> bool {
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
        return false;
    };
    let options = match manager.source_options(&DimensionSourceOptionsRequest {
        sheet: &operation.sheet,
        actual_type: &operation.actual_type,
    }) {
        Ok(options) => options,
        Err(error) => {
            diagnostics.extend(error);
            return false;
        }
    };
    let source =
        dimension_resolved_source(project, &operation.path, &operation.provider_id, options);
    if let Err(error) =
        transaction.snapshot_file(&operation.path, &operation.dimension, &project.config_path)
    {
        diagnostics.extend(error);
        return false;
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
            true
        }
        Ok(_) => false,
        Err(error) => {
            diagnostics.extend(error);
            false
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct DimensionGenerationPlanResult {
    pub(super) plan: DimensionGenerationPlan,
    pub(super) diagnostics: DiagnosticSet,
}

#[derive(Debug, Default)]
pub(crate) struct DimensionGenerationPlan {
    pub(super) operations: Vec<DimensionGenerationPlanOp>,
}

#[derive(Debug)]
pub(super) enum DimensionGenerationPlanOp {
    Move { from: PathBuf, to: PathBuf },
    Remove(PathBuf),
    Sync(DimensionGenerationOperation),
}

#[derive(Debug)]
pub(super) struct DimensionGenerationOperation {
    pub(super) dimension: String,
    pub(super) provider_id: String,
    pub(super) path: PathBuf,
    pub(super) sheet: String,
    pub(super) actual_type: String,
    pub(super) entries: Vec<DimensionSourceEntry>,
    pub(super) variants: Vec<String>,
    pub(super) bucket: String,
    pub(super) is_singleton: bool,
}

impl DimensionGenerationOperation {
    pub(super) fn matches_renamed_source(&self, path: &Path) -> bool {
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
    pub planned_sources: usize,
    pub written_sources: usize,
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
        location: SourceLocationSpec::new(path.to_path_buf()),
        options,
        display_name,
    }
}

pub(super) fn dimension_diagnostic(
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
        contexts: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::{
        commit_dimension_generation, plan_dimension_generation_scoped,
        DimensionGenerationOperation, DimensionGenerationPlan, DimensionGenerationPlanOp,
        DimensionGenerationTransaction,
    };
    use coflow_api::ProviderRegistry;
    use coflow_cft::{
        BucketName, CftDimensionInputs, CftFile, DimensionName, FieldName, ModuleId, TypeName,
    };
    use coflow_data_model::{CfdDataModel, LoadedValueDraft};
    use coflow_project::Project;

    use crate::dimensions::DimensionField;

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
    fn scoped_generation_plans_only_affected_fields() {
        let root = std::env::temp_dir().join(format!(
            "coflow-runtime-dimension-scoped-plan-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("create temp dir");
        std::fs::write(
            root.join("schema.cft"),
            "type Item { name: string; } type Other { label: string; }",
        )
        .expect("write schema");
        std::fs::write(
            root.join("coflow.yaml"),
            "schema: schema.cft\nsources: []\ndimensions:\n  language:\n    variants: [zh]\n    out_dir: dimensions/language\n",
        )
        .expect("write config");
        let project = Project::open_schema_only(Some(&root)).expect("open project");
        let modules = coflow_cft::parse_modules([CftFile::new(
            ModuleId::from("schema.cft"),
            "schema.cft".into(),
            "type Item { name: string; } type Other { label: string; }",
        )]);
        let dimensions = CftDimensionInputs::try_new([("language", vec!["zh".to_string()])])
            .expect("dimensions");
        let schema = coflow_cft::build_schema(&modules, &dimensions).expect("schema");
        let mut builder = CfdDataModel::builder(&schema);
        builder.add_record("item", "Item", [("name", LoadedValueDraft::from("Item"))]);
        builder.add_record(
            "other",
            "Other",
            [("label", LoadedValueDraft::from("Other"))],
        );
        let model = builder.build().expect("model");
        let language = DimensionName::new("language").expect("dimension");
        let fields = vec![
            DimensionField {
                dimension: language.clone(),
                source_type: TypeName::new("Item").expect("type"),
                source_field: FieldName::new("name").expect("field"),
                bucket: BucketName::new("Item").expect("bucket"),
                is_singleton: false,
            },
            DimensionField {
                dimension: language,
                source_type: TypeName::new("Other").expect("type"),
                source_field: FieldName::new("label").expect("field"),
                bucket: BucketName::new("Other").expect("bucket"),
                is_singleton: false,
            },
        ];

        let result = plan_dimension_generation_scoped(
            &project,
            &schema,
            &model,
            &fields,
            Some(&std::collections::BTreeSet::from([0])),
        );

        assert!(result.diagnostics.is_empty(), "{:#?}", result.diagnostics);
        assert_eq!(result.plan.operations.len(), 1);
        assert!(matches!(
            &result.plan.operations[0],
            DimensionGenerationPlanOp::Sync(operation) if operation.actual_type == "Item"
        ));
        std::fs::remove_dir_all(root).expect("remove temp dir");
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
