mod plan;

use crate::api::{
    CfdSource, CfdSourceCatalog, CfdSourcePath, Diagnostic, DiagnosticSet, DimensionSourceEntry,
    DimensionSourceRequest, Label, Severity, SourceLocation,
};
use crate::data_model::CfdDataModel;
use crate::dimensions::DimensionField;
use crate::cfd_loader::CfdWriter;
use crate::project::Project;
use coflow_language::CftSchema;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use plan::plan_dimension_generation_scoped;

#[must_use]
pub(crate) fn regenerate_dimension_sources_scoped(
    project: &Project,
    schema: &CftSchema,
    model: &CfdDataModel,
    fields: &[DimensionField],
    affected_fields: Option<&BTreeSet<usize>>,
    catalog: &CfdSourceCatalog,
) -> DimensionGenerationResult {
    let plan_result =
        plan_dimension_generation_scoped(project, schema, model, fields, affected_fields);
    let planned_sources = plan_result.plan.operations.len();
    if !plan_result.diagnostics.is_empty() {
        return DimensionGenerationResult {
            diagnostics: plan_result.diagnostics,
            planned_sources,
            writer: None,
            changed_paths: Vec::new(),
            written_sources: 0,
        };
    }
    let mut result = commit_dimension_generation(project, plan_result.plan, catalog);
    let mut diagnostics = plan_result.diagnostics;
    diagnostics.extend(result.diagnostics);
    result.diagnostics = diagnostics;
    result.planned_sources = planned_sources;
    result
}

fn commit_dimension_generation(
    project: &Project,
    plan: DimensionGenerationPlan,
    catalog: &CfdSourceCatalog,
) -> DimensionGenerationResult {
    let mut diagnostics = DiagnosticSet::empty();
    let mut changed_paths = BTreeSet::new();
    let planned_sources = plan.operations.len();
    let mut written_sources = 0_usize;

    for operation in plan.operations {
        let changed = match operation {
            DimensionGenerationPlanOp::Move { from, to } => commit_dimension_move(
                catalog,
                from,
                to,
                &mut diagnostics,
                &mut changed_paths,
            ),
            DimensionGenerationPlanOp::Remove(path) => commit_dimension_remove(
                catalog,
                path,
                &mut diagnostics,
                &mut changed_paths,
            ),
            DimensionGenerationPlanOp::Sync(operation) => commit_dimension_sync(
                project,
                &catalog.writer(),
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
        writer: Some(catalog.writer()),
        diagnostics,
        changed_paths: changed_paths.into_iter().collect(),
        planned_sources,
        written_sources,
    }
}

fn commit_dimension_move(
    catalog: &CfdSourceCatalog,
    from: PathBuf,
    to: PathBuf,
    diagnostics: &mut DiagnosticSet,
    changed_paths: &mut BTreeSet<PathBuf>,
) -> bool {
    let writer = catalog.writer();
    if let Err(error) = writer.move_source(&from, &to) {
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
    catalog: &CfdSourceCatalog,
    path: PathBuf,
    diagnostics: &mut DiagnosticSet,
    changed_paths: &mut BTreeSet<PathBuf>,
) -> bool {
    let writer = catalog.writer();
    if let Err(error) = writer.delete_source(&path) {
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
    writer: &Arc<CfdWriter>,
    operation: DimensionGenerationOperation,
    diagnostics: &mut DiagnosticSet,
    changed_paths: &mut BTreeSet<PathBuf>,
) -> bool {
    let source = dimension_resolved_source(project, &operation.path);
    let result = writer.sync_dimension_source(&DimensionSourceRequest {
        source: &source,
        entries: &operation.entries,
        variants: &operation.variants,
    });
    match result {
        Ok(result) if result.changed => {
            changed_paths.insert(operation.path);
            true
        }
        Ok(_) => false,
        Err(error) => {
            for diagnostic in error.diagnostics {
                diagnostics.push(Diagnostic {
                    code: "DIM-SOURCE-006".to_string(),
                    ..diagnostic
                });
            }
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
    pub(super) path: PathBuf,
    pub(super) actual_type: String,
    pub(super) entries: Vec<DimensionSourceEntry>,
    pub(super) variants: Vec<String>,
    pub(super) bucket: String,
    pub(super) is_singleton: bool,
}

impl DimensionGenerationOperation {
    pub(super) fn matches_renamed_source(&self, path: &Path) -> bool {
        !self.is_singleton
            && path.extension().and_then(|extension| extension.to_str()) == Some("cfd")
            && path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .is_some_and(|stem| stem.starts_with(&format!("{}_", self.bucket)))
    }
}

#[derive(Debug, Default)]
pub struct DimensionGenerationResult {
    pub writer: Option<Arc<CfdWriter>>,
    pub diagnostics: DiagnosticSet,
    pub changed_paths: Vec<PathBuf>,
    pub planned_sources: usize,
    pub written_sources: usize,
}

fn dimension_resolved_source(project: &Project, path: &Path) -> CfdSource {
    let display_name = path.strip_prefix(project.root_dir()).map_or_else(
        |_| path.display().to_string(),
        crate::project::path_to_slash,
    );
    CfdSource {
        location: CfdSourcePath::new(path.to_path_buf()),
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
    };
    use crate::catalog::CfdSourceCatalog;
    use crate::data_model::{CfdDataModel, LoadedValueDraft};
    use crate::project::Project;
    use coflow_language::{
        BucketName, CftDimensionInputs, CftFile, DimensionName, FieldName, ModuleId, TypeName,
    };

    use crate::dimensions::DimensionField;

    fn test_project(root: &std::path::Path) -> Project {
        std::fs::write(root.join("schema.cft"), "type Item { name: string; }")
            .expect("write schema");
        std::fs::write(
            root.join("coflow.yaml"),
            "schema: schema.cft\ndata: []\ncodegen:\n  - language: csharp\n    dir: generated/csharp\n",
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
            "schema: schema.cft\ndata: []\ncodegen:\n  - language: csharp\n    dir: generated/csharp\ndimensions:\n  language:\n    variants: [zh]\n    out_dir: dimensions/language\n",
        )
        .expect("write config");
        let project = Project::open_schema_only(Some(&root)).expect("open project");
        let modules = coflow_language::parse_modules([CftFile::new(
            ModuleId::from("schema.cft"),
            "schema.cft".into(),
            "type Item { name: string; } type Other { label: string; }",
        )]);
        let dimensions = CftDimensionInputs::try_new([("language", vec!["zh".to_string()])])
            .expect("dimensions");
        let schema = coflow_language::build_schema(&modules, &dimensions).expect("schema");
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
    fn generation_operation_failures_report_stable_codes() {
        let root = std::env::temp_dir().join(format!(
            "coflow-runtime-dimension-operation-errors-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("create temp dir");
        let project = test_project(&root);
        let missing_move = root.join("missing-old.cfd");
        let missing_remove = root.join("missing-stale.cfd");
        let generated = root.join("generated.cfd");
        std::fs::create_dir(&generated).expect("create invalid generated source");
        let plan = DimensionGenerationPlan {
            operations: vec![
                DimensionGenerationPlanOp::Move {
                    from: missing_move,
                    to: root.join("moved.cfd"),
                },
                DimensionGenerationPlanOp::Remove(missing_remove),
                DimensionGenerationPlanOp::Sync(DimensionGenerationOperation {
                    path: generated,
                    actual_type: "Item".to_string(),
                    entries: Vec::new(),
                    variants: vec!["zh".to_string()],
                    bucket: "Item".to_string(),
                    is_singleton: false,
                }),
            ],
        };

        let result = commit_dimension_generation(&project, plan, &CfdSourceCatalog::default());
        let codes = result
            .diagnostics
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        assert!(codes.contains("DIM-SOURCE-005"));
        assert!(codes.contains("DIM-SOURCE-006"));
        std::fs::remove_dir_all(root).expect("remove temp dir");
    }

}
