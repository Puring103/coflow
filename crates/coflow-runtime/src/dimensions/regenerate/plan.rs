use super::{
    dimension_diagnostic, DimensionGenerationOperation, DimensionGenerationPlan,
    DimensionGenerationPlanOp, DimensionGenerationPlanResult,
};
use crate::api::DiagnosticSet;
use crate::data_model::{CfdDataModel, CfdValue};
use crate::dimensions::DimensionField;
use crate::project::Project;
use coflow_language::cft::CftSchema;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

#[must_use]
pub(super) fn plan_dimension_generation_scoped(
    project: &Project,
    schema: &CftSchema,
    model: &CfdDataModel,
    fields: &[DimensionField],
    affected_fields: Option<&BTreeSet<usize>>,
) -> DimensionGenerationPlanResult {
    let mut diagnostics = DiagnosticSet::empty();
    let mut operations = Vec::new();
    for (dimension, config) in &project.config().dimensions {
        let result = plan_configured_dimension(
            project,
            schema,
            model,
            fields,
            affected_fields,
            dimension,
            config,
        );
        operations.extend(result.plan.operations);
        diagnostics.extend(result.diagnostics);
    }
    DimensionGenerationPlanResult {
        plan: DimensionGenerationPlan { operations },
        diagnostics,
    }
}

fn plan_configured_dimension(
    project: &Project,
    schema: &CftSchema,
    model: &CfdDataModel,
    fields: &[DimensionField],
    affected_fields: Option<&BTreeSet<usize>>,
    dimension: &str,
    config: &crate::project::DimensionConfig,
) -> DimensionGenerationPlanResult {
    let mut diagnostics = DiagnosticSet::empty();
    let Some(out_dir) = config.out_dir.as_ref() else {
        diagnostics.push(dimension_diagnostic(
            project.config_path(),
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
    for (field_index, field) in fields
        .iter()
        .enumerate()
        .filter(|(_, field)| field.dimension.as_str() == dimension)
    {
        let path = out_dir.join(field.source_file_name());
        let path_identity = crate::project::normalized_path_identity(&path);
        expected_paths.insert(path_identity.clone());
        if affected_fields.is_some_and(|affected| !affected.contains(&field_index)) {
            continue;
        }
        let operation = DimensionGenerationOperation {
            path: path.clone(),
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
                    project.config_path(),
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
        project.config_path(),
        dimension,
        &out_dir,
        &expected_paths,
        &dimension_operations,
    ) {
        Ok(operations) => operations,
        Err(error) => {
            return DimensionGenerationPlanResult {
                plan: DimensionGenerationPlan::default(),
                diagnostics: {
                    diagnostics.extend(error);
                    diagnostics
                },
            }
        }
    };
    DimensionGenerationPlanResult {
        plan: DimensionGenerationPlan {
            operations: reconciliations
                .into_iter()
                .chain(
                    dimension_operations
                        .into_iter()
                        .map(DimensionGenerationPlanOp::Sync),
                )
                .collect(),
        },
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
            )))
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
                .is_some_and(|extension| extension == "cfd")
                && !expected_paths.contains(&crate::project::normalized_path_identity(path))
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

fn dimension_entries(
    schema: &CftSchema,
    model: &CfdDataModel,
    field: &DimensionField,
) -> Vec<crate::api::DimensionSourceEntry> {
    if field.is_singleton {
        model
            .records_assignable_to(schema, &field.source_type)
            .next()
            .map(|(_, record)| crate::api::DimensionSourceEntry {
                key: field.source_field.to_string(),
                actual_type: field.source_type.to_string(),
                default: record
                    .fields()
                    .get(field.source_field.as_str())
                    .cloned()
                    .unwrap_or(CfdValue::OptionNone),
            })
            .into_iter()
            .collect()
    } else {
        model
            .records_assignable_to(schema, &field.source_type)
            .map(|(_, record)| crate::api::DimensionSourceEntry {
                key: record.key().to_string(),
                actual_type: field.source_type.to_string(),
                default: record
                    .fields()
                    .get(field.source_field.as_str())
                    .cloned()
                    .unwrap_or(CfdValue::OptionNone),
            })
            .collect()
    }
}
