use std::collections::BTreeSet;

use coflow_cft::CftSchema;
use coflow_data_model::{CfdDataModel, CfdRecordId};

use crate::snapshot::CheckRoot;
use crate::{
    CheckExecutionStats, CheckOutput, CheckRequest, CheckRound, CheckSnapshot, CheckTargets,
    DependencyCollection, DependencyGraph, DimensionCheckContext, DimensionCheckRound,
    RootedCheckDiagnostic, StructuralLimits,
};

mod evaluator;
mod expressions;
mod runner;
mod statements;

use crate::dependencies as deps;
use crate::diagnostics;
use crate::CheckDiagnosticContext;
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
    let selection = select_targets(model, request.targets, &request.rounds);
    let requested_roots = selection.requested_roots();
    let executed_rounds = selection.executed_rounds();
    if executed_rounds == 0 {
        return CheckOutput {
            snapshot: selection
                .previous
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

    if !selection.default_targets.is_empty() {
        let execution = run_default_round(
            schema,
            model,
            &selection.default_targets,
            collect_dependencies,
            request.structural_limits,
        );
        record_execution(
            &mut replacement,
            model,
            &CheckRound::Default,
            &selection.default_targets,
            &execution,
        );
        diagnostics.extend(execution.diagnostics);
        dependencies.merge(execution.dependencies);
    }

    for (round, targets) in &selection.dimension_targets {
        if targets.is_empty() {
            continue;
        }
        let execution = run_dimension_round(
            schema,
            model,
            round,
            targets,
            collect_dependencies,
            request.structural_limits,
        );
        dimension_projected_records += execution.dimension_projected_records;
        record_execution(
            &mut replacement,
            model,
            &CheckRound::Dimension(round.clone()),
            targets,
            &execution,
        );
        diagnostics.extend(execution.diagnostics);
        dependencies.merge(execution.dependencies);
    }

    let snapshot = replacement.map(|replacement| match selection.previous {
        Some(previous) => {
            CheckSnapshot::merge_replacement(previous, replacement, &selection.replaced)
        }
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

struct TargetSelection<'a> {
    default_targets: Vec<CfdRecordId>,
    dimension_targets: Vec<(DimensionCheckRound, Vec<CfdRecordId>)>,
    previous: Option<&'a CheckSnapshot>,
    replaced: BTreeSet<CheckRoot>,
}

impl TargetSelection<'_> {
    fn requested_roots(&self) -> usize {
        self.default_targets
            .iter()
            .copied()
            .chain(
                self.dimension_targets
                    .iter()
                    .flat_map(|(_, targets)| targets.iter().copied()),
            )
            .collect::<BTreeSet<_>>()
            .len()
    }

    fn executed_rounds(&self) -> usize {
        self.default_targets.len()
            + self
                .dimension_targets
                .iter()
                .map(|(_, targets)| targets.len())
                .sum::<usize>()
    }
}

fn select_targets<'a>(
    model: &CfdDataModel,
    targets: CheckTargets<'a>,
    rounds: &[DimensionCheckRound],
) -> TargetSelection<'a> {
    match targets {
        CheckTargets::All => full_selection(model, rounds),
        CheckTargets::Records(targets) => {
            selection_for_targets(deduplicate_preserving_order(targets.to_vec()), rounds)
        }
        CheckTargets::Incremental { previous, changed } => {
            select_incremental_targets(model, previous, changed, rounds)
                .unwrap_or_else(|| full_selection(model, rounds))
        }
    }
}

fn full_selection<'a>(model: &CfdDataModel, rounds: &[DimensionCheckRound]) -> TargetSelection<'a> {
    let targets = model.records().map(|(id, _)| id).collect();
    selection_for_targets(targets, rounds)
}

fn selection_for_targets<'a>(
    targets: Vec<CfdRecordId>,
    rounds: &[DimensionCheckRound],
) -> TargetSelection<'a> {
    let dimension_targets = rounds
        .iter()
        .cloned()
        .map(|round| (round, targets.clone()))
        .collect();
    TargetSelection {
        default_targets: targets,
        dimension_targets,
        previous: None,
        replaced: BTreeSet::new(),
    }
}

fn select_incremental_targets<'a>(
    model: &CfdDataModel,
    previous: &'a CheckSnapshot,
    changed: &BTreeSet<coflow_data_model::RecordCoordinate>,
    rounds: &[DimensionCheckRound],
) -> Option<TargetSelection<'a>> {
    let changed = expand_materialization_changes(model, changed);
    let replaced = previous.affected_roots(&changed, rounds)?;
    let mut default_targets = Vec::new();
    let mut dimension_targets = rounds
        .iter()
        .cloned()
        .map(|round| (round, Vec::new()))
        .collect::<Vec<_>>();
    for root in &replaced {
        let id = model.record_by_type_key(&root.record.actual_type, &root.record.key)?;
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
    Some(TargetSelection {
        default_targets,
        dimension_targets,
        previous: Some(previous),
        replaced,
    })
}

struct RoundExecution {
    diagnostics: Vec<RootedCheckDiagnostic>,
    dependencies: DependencyGraph,
    dimension_projected_records: usize,
}

fn run_default_round(
    schema: &CftSchema,
    model: &CfdDataModel,
    targets: &[CfdRecordId],
    collect_dependencies: bool,
    structural_limits: StructuralLimits,
) -> RoundExecution {
    let (diagnostics, dependencies, _) = CheckRunner::new(schema, model, structural_limits)
        .run_rooted(targets, collect_dependencies);
    RoundExecution {
        diagnostics: diagnostics
            .into_iter()
            .map(|(root, diagnostic)| RootedCheckDiagnostic { root, diagnostic })
            .collect(),
        dependencies,
        dimension_projected_records: 0,
    }
}

fn run_dimension_round(
    schema: &CftSchema,
    model: &CfdDataModel,
    round: &DimensionCheckRound,
    targets: &[CfdRecordId],
    collect_dependencies: bool,
    structural_limits: StructuralLimits,
) -> RoundExecution {
    let context = DimensionCheckContext {
        dimension: round.dimension.clone(),
        variant: round.variant.clone(),
    };
    let (diagnostics, dependencies, dimension_projected_records) =
        CheckRunner::with_dimension_context(schema, model, context, structural_limits)
            .run_rooted(targets, collect_dependencies);
    let diagnostics = diagnostics
        .into_iter()
        .map(|(root, mut diagnostic)| {
            dimensions::attach_dimension_origins(model, round, &mut diagnostic.diagnostic);
            diagnostic.contexts.insert(
                0,
                CheckDiagnosticContext::Dimension {
                    dimension: round.dimension.to_string(),
                    variant: round.variant.to_string(),
                },
            );
            RootedCheckDiagnostic { root, diagnostic }
        })
        .collect();
    RoundExecution {
        diagnostics,
        dependencies,
        dimension_projected_records,
    }
}

fn record_execution(
    replacement: &mut Option<CheckSnapshot>,
    model: &CfdDataModel,
    round: &CheckRound,
    targets: &[CfdRecordId],
    execution: &RoundExecution,
) {
    if replacement.as_mut().is_some_and(|snapshot| {
        snapshot
            .insert_execution(
                model,
                round,
                targets,
                execution.diagnostics.clone(),
                &execution.dependencies,
            )
            .is_none()
    }) {
        *replacement = None;
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
