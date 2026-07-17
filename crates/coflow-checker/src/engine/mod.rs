use std::collections::BTreeSet;

use coflow_cft::CftSchema;
use coflow_data_model::{CfdDataModel, CfdRecordId};

use crate::{
    CheckExecutionStats, CheckOutput, CheckRequest, CheckRound, CheckSnapshot, CheckTargets,
    DependencyCollection, DependencyGraph, DimensionCheckContext, RootedCheckDiagnostic,
};

mod evaluator;
mod expressions;
mod runner;
mod statements;

use crate::dependencies as deps;
use crate::diagnostics;
use crate::diagnostics::{explanations, trace as evaluation_trace};
use crate::dimensions;
use crate::eval as value;
use crate::operations::{
    access, builtins, comparison as ops, predicates as type_predicates, quantifiers,
};

pub(crate) use runner::CheckRunner;

pub(crate) fn execute(
    schema: &CftSchema,
    model: &CfdDataModel,
    mut request: CheckRequest<'_>,
) -> CheckOutput {
    request.rounds = deduplicate_preserving_order(request.rounds);
    let collect_dependencies = request.dependency_collection == DependencyCollection::Reads;
    let all_targets = || model.records().map(|(id, _)| id).collect::<Vec<_>>();
    let (default_targets, dimension_targets, previous, replaced) = match request.targets {
        CheckTargets::All => {
            let targets = all_targets();
            let dimensions = request
                .rounds
                .iter()
                .cloned()
                .map(|round| (round, targets.clone()))
                .collect::<Vec<_>>();
            (targets, dimensions, None, BTreeSet::new())
        }
        CheckTargets::Records(targets) => {
            let targets = deduplicate_preserving_order(targets.to_vec());
            let dimensions = request
                .rounds
                .iter()
                .cloned()
                .map(|round| (round, targets.clone()))
                .collect::<Vec<_>>();
            (targets, dimensions, None, BTreeSet::new())
        }
        CheckTargets::Incremental { previous, changed } => {
            let changed = expand_materialization_changes(model, changed);
            let selection = previous
                .affected_roots(&changed, &request.rounds)
                .and_then(|roots| {
                    let mut default_targets = Vec::new();
                    let mut dimension_targets = request
                        .rounds
                        .iter()
                        .cloned()
                        .map(|round| (round, Vec::new()))
                        .collect::<Vec<_>>();
                    for root in &roots {
                        let id =
                            model.record_by_type_key(&root.record.actual_type, &root.record.key)?;
                        match &root.round {
                            CheckRound::Default => default_targets.push(id),
                            CheckRound::Dimension(round) => {
                                if let Some((_, targets)) = dimension_targets
                                    .iter_mut()
                                    .find(|(candidate, _)| candidate == round)
                                {
                                    targets.push(id);
                                }
                            }
                        }
                    }
                    Some((default_targets, dimension_targets, roots))
                });
            match selection {
                Some((default_targets, dimension_targets, replaced)) => {
                    (default_targets, dimension_targets, Some(previous), replaced)
                }
                None => {
                    let targets = all_targets();
                    let dimensions = request
                        .rounds
                        .iter()
                        .cloned()
                        .map(|round| (round, targets.clone()))
                        .collect::<Vec<_>>();
                    (targets, dimensions, None, BTreeSet::new())
                }
            }
        }
    };

    let requested_roots = default_targets
        .iter()
        .copied()
        .chain(
            dimension_targets
                .iter()
                .flat_map(|(_, targets)| targets.iter().copied()),
        )
        .collect::<BTreeSet<CfdRecordId>>()
        .len();
    let executed_rounds = default_targets.len()
        + dimension_targets
            .iter()
            .map(|(_, targets)| targets.len())
            .sum::<usize>();
    if executed_rounds == 0 {
        return CheckOutput {
            snapshot: previous
                .cloned()
                .or_else(|| collect_dependencies.then(CheckSnapshot::stable_empty)),
            statistics: CheckExecutionStats {
                dependency_collection: request.dependency_collection,
                ..CheckExecutionStats::default()
            },
            ..CheckOutput::default()
        };
    }

    let mut replacement = collect_dependencies.then(CheckSnapshot::stable_empty);
    let mut diagnostics = Vec::new();
    let mut dependencies = DependencyGraph::default();
    let mut dimension_projected_records = 0;

    if !default_targets.is_empty() {
        let (raw_diagnostics, round_dependencies, _) =
            CheckRunner::new(schema, model, request.structural_limits)
                .run_rooted(&default_targets, collect_dependencies);
        let rooted = raw_diagnostics
            .into_iter()
            .map(|(root, diagnostic)| RootedCheckDiagnostic { root, diagnostic })
            .collect::<Vec<_>>();
        if replacement.as_mut().is_some_and(|snapshot| {
            snapshot
                .insert_execution(
                    model,
                    CheckRound::Default,
                    &default_targets,
                    rooted.clone(),
                    &round_dependencies,
                )
                .is_none()
        }) {
            replacement = None;
        }
        diagnostics.extend(rooted);
        dependencies.merge(round_dependencies);
    }

    for (round, targets) in &dimension_targets {
        if targets.is_empty() {
            continue;
        }
        let context = DimensionCheckContext {
            dimension: round.dimension.clone(),
            variant: round.variant.clone(),
        };
        let (round_diagnostics, round_dependencies, projected_records) =
            CheckRunner::with_dimension_context(schema, model, context, request.structural_limits)
                .run_rooted(targets, collect_dependencies);
        dimension_projected_records += projected_records;
        let rooted = round_diagnostics
            .into_iter()
            .map(|(root, mut diagnostic)| {
                dimensions::attach_dimension_origins(model, round, &mut diagnostic);
                diagnostic.message = format!(
                    "[{}={}] {}",
                    round.dimension, round.variant, diagnostic.message
                );
                RootedCheckDiagnostic { root, diagnostic }
            })
            .collect::<Vec<_>>();
        if replacement.as_mut().is_some_and(|snapshot| {
            snapshot
                .insert_execution(
                    model,
                    CheckRound::Dimension(round.clone()),
                    targets,
                    rooted.clone(),
                    &round_dependencies,
                )
                .is_none()
        }) {
            replacement = None;
        }
        diagnostics.extend(rooted);
        dependencies.merge(round_dependencies);
    }

    let snapshot = replacement.map(|replacement| match previous {
        Some(previous) => CheckSnapshot::merge_replacement(previous, replacement, &replaced),
        None => replacement,
    });

    CheckOutput {
        diagnostics,
        dependencies,
        snapshot,
        statistics: CheckExecutionStats {
            requested_roots,
            executed_rounds,
            dimension_projected_records,
            dependency_collection: request.dependency_collection,
        },
    }
}

fn deduplicate_preserving_order<T: Clone + Ord>(values: Vec<T>) -> Vec<T> {
    let mut seen = BTreeSet::new();
    values
        .into_iter()
        .filter(|value| seen.insert(value.clone()))
        .collect()
}

fn expand_materialization_changes(
    model: &CfdDataModel,
    changed: &BTreeSet<coflow_data_model::RecordCoordinate>,
) -> BTreeSet<coflow_data_model::RecordCoordinate> {
    let source_ids = changed.iter().filter_map(|coordinate| {
        model.record_by_type_key(&coordinate.actual_type, &coordinate.key)
    });
    let mut expanded = changed.clone();
    expanded.extend(
        model
            .materialization_dependents(source_ids)
            .into_iter()
            .filter_map(|id| {
                model
                    .record(id)
                    .map(coflow_data_model::CfdRecord::coordinate)
            }),
    );
    expanded
}
