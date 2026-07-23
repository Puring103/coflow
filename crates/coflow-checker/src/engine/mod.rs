use std::collections::BTreeSet;

use coflow_cft::CftSchema;
use coflow_data_model::{CfdDataModel, CfdRecordId};

use crate::snapshot::{CheckRoot, StableExecutionId};
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
    let selection = select_targets(schema, model, request.targets, &request.rounds);
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

    if !selection.top_level_default_targets.is_empty() {
        let execution = run_top_level_checks(
            schema,
            model,
            &selection.top_level_default_targets,
            None,
            collect_dependencies,
            request.structural_limits,
        );
        record_top_level_execution(
            &mut replacement,
            model,
            &CheckRound::Default,
            &selection.top_level_default_targets,
            &execution,
        );
        diagnostics.extend(execution.diagnostics);
        dependencies.merge(execution.dependencies);
    }

    for (round, targets) in &selection.top_level_dimension_targets {
        if targets.is_empty() {
            continue;
        }
        let execution = run_top_level_checks(
            schema,
            model,
            targets,
            Some(round),
            collect_dependencies,
            request.structural_limits,
        );
        record_top_level_execution(
            &mut replacement,
            model,
            &CheckRound::Dimension(round.clone()),
            targets,
            &execution,
        );
        diagnostics.extend(execution.diagnostics);
        dependencies.merge(execution.dependencies);
    }

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
            executed_top_level_checks: selection.top_level_execution_count(),
            dependency_collection: request.dependency_collection,
        },
    }
}

struct TargetSelection<'a> {
    top_level_default_targets: Vec<coflow_cft::CheckName>,
    top_level_dimension_targets: Vec<(DimensionCheckRound, Vec<coflow_cft::CheckName>)>,
    default_targets: Vec<CfdRecordId>,
    dimension_targets: Vec<(DimensionCheckRound, Vec<CfdRecordId>)>,
    previous: Option<&'a CheckSnapshot>,
    replaced: BTreeSet<CheckRoot>,
}

impl TargetSelection<'_> {
    fn requested_roots(&self) -> usize {
        self.top_level_execution_count() + self.default_targets
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
        self.top_level_execution_count() + self.default_targets.len()
            + self
                .dimension_targets
                .iter()
                .map(|(_, targets)| targets.len())
                .sum::<usize>()
    }

    fn top_level_execution_count(&self) -> usize {
        self.top_level_default_targets.len()
            + self
                .top_level_dimension_targets
                .iter()
                .map(|(_, targets)| targets.len())
                .sum::<usize>()
    }
}

fn select_targets<'a>(
    schema: &CftSchema,
    model: &CfdDataModel,
    targets: CheckTargets<'a>,
    rounds: &[DimensionCheckRound],
) -> TargetSelection<'a> {
    match targets {
        CheckTargets::All => full_selection(schema, model, rounds),
        CheckTargets::Records(targets) => {
            selection_for_targets(deduplicate_preserving_order(targets.to_vec()), rounds)
        }
        CheckTargets::Incremental { previous, changed } => {
            select_incremental_targets(model, previous, changed, rounds)
                .unwrap_or_else(|| full_selection(schema, model, rounds))
        }
    }
}

fn full_selection<'a>(schema: &CftSchema, model: &CfdDataModel, rounds: &[DimensionCheckRound]) -> TargetSelection<'a> {
    let targets = model.records().map(|(id, _)| id).collect();
    let mut selection = selection_for_targets(targets, rounds);
    selection.top_level_default_targets =
        schema.all_checks().map(|check| check.name.clone()).collect();
    selection.top_level_dimension_targets = rounds
        .iter()
        .cloned()
        .map(|round| {
            let targets = schema
                .all_checks()
                .filter(|check| check.statement_indices(&round.dimension).is_some())
                .map(|check| check.name.clone())
                .collect();
            (round, targets)
        })
        .collect();
    selection
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
        top_level_default_targets: Vec::new(),
        top_level_dimension_targets: rounds
            .iter()
            .cloned()
            .map(|round| (round, Vec::new()))
            .collect(),
        default_targets: targets,
        dimension_targets,
        previous: None,
        replaced: BTreeSet::new(),
    }
}

fn select_incremental_targets<'a>(
    model: &CfdDataModel,
    previous: &'a CheckSnapshot,
    changed: &crate::CheckChangeSet,
    rounds: &[DimensionCheckRound],
) -> Option<TargetSelection<'a>> {
    let changed = expand_materialization_changes(model, changed);
    let replaced = previous.affected_roots(&changed, rounds)?;
    let mut default_targets = Vec::new();
    let mut top_level_default_targets = Vec::new();
    let mut top_level_dimension_targets = rounds
        .iter()
        .cloned()
        .map(|round| (round, Vec::new()))
        .collect::<Vec<_>>();
    let mut dimension_targets = rounds
        .iter()
        .cloned()
        .map(|round| (round, Vec::new()))
        .collect::<Vec<_>>();
    for root in &replaced {
        let id = match &root.execution {
            StableExecutionId::Record(record) => {
                Some(model.record_by_type_key(&record.actual_type, &record.key)?)
            }
            StableExecutionId::TopLevel(name) => {
                match &root.round {
                    CheckRound::Default => top_level_default_targets.push(name.clone()),
                    CheckRound::Dimension(round) => {
                        if let Some((_, targets)) = top_level_dimension_targets
                            .iter_mut()
                            .find(|(candidate, _)| candidate == round)
                        {
                            targets.push(name.clone());
                        }
                    }
                }
                None
            }
        };
        let Some(id) = id else { continue; };
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
        top_level_default_targets,
        top_level_dimension_targets,
        default_targets,
        dimension_targets,
        previous: Some(previous),
        replaced,
    })
}

fn run_top_level_checks(
    schema: &CftSchema,
    model: &CfdDataModel,
    targets: &[coflow_cft::CheckName],
    round: Option<&DimensionCheckRound>,
    collect_dependencies: bool,
    structural_limits: StructuralLimits,
) -> RoundExecution {
    let runner = round.map_or_else(
        || CheckRunner::new(schema, model, structural_limits),
        |round| {
            CheckRunner::with_dimension_context(
                schema,
                model,
                DimensionCheckContext {
                    dimension: round.dimension.clone(),
                    variant: round.variant.clone(),
                },
                structural_limits,
            )
        },
    );
    let (diagnostics, dependencies) = runner.run_top_level(targets, collect_dependencies);
    let diagnostics = diagnostics
        .into_iter()
        .map(|(root, mut diagnostic)| {
            if let Some(round) = round {
                dimensions::attach_dimension_origins(model, round, &mut diagnostic.diagnostic);
                diagnostic.contexts.insert(
                    0,
                    CheckDiagnosticContext::Dimension {
                        dimension: round.dimension.to_string(),
                        variant: round.variant.to_string(),
                    },
                );
            }
            RootedCheckDiagnostic { root, diagnostic }
        })
        .collect();
    RoundExecution {
        diagnostics,
        dependencies,
        dimension_projected_records: 0,
    }
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
    let execution_targets = targets
        .iter()
        .copied()
        .map(crate::CheckExecutionId::Record)
        .collect::<Vec<_>>();
    if replacement.as_mut().is_some_and(|snapshot| {
        snapshot
            .insert_execution(
                model,
                round,
                &execution_targets,
                execution.diagnostics.clone(),
                &execution.dependencies,
            )
            .is_none()
    }) {
        *replacement = None;
    }
}

fn record_top_level_execution(
    replacement: &mut Option<CheckSnapshot>,
    model: &CfdDataModel,
    round: &CheckRound,
    targets: &[coflow_cft::CheckName],
    execution: &RoundExecution,
) {
    let execution_targets = targets
        .iter()
        .cloned()
        .map(crate::CheckExecutionId::TopLevel)
        .collect::<Vec<_>>();
    if replacement.as_mut().is_some_and(|snapshot| {
        snapshot
            .insert_execution(
                model,
                round,
                &execution_targets,
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
    changed: &crate::CheckChangeSet,
) -> crate::CheckChangeSet {
    let source_ids = changed.records.keys().filter_map(|coordinate| {
        model.record_by_type_key(&coordinate.actual_type, &coordinate.key)
    });
    let mut expanded = changed.clone();
    let dependents = model
            .materialization_dependents(source_ids)
            .into_iter()
            .filter_map(|id| {
                model
                    .record(id)
                    .map(coflow_data_model::CfdRecord::coordinate)
            });
    for coordinate in dependents {
        expanded
            .records
            .entry(coordinate)
            .or_insert(crate::ChangedPaths::All);
    }
    expanded
}
