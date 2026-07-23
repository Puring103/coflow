use std::collections::{BTreeMap, BTreeSet};

use coflow_data_model::{
    CfdDataModel, CfdDiagnostic, CfdLabel, CfdPath, CfdRecordId, CfdSeverity, CfdStage,
    RecordCoordinate, RecordOrigin,
};

use crate::{
    CheckDiagnostic, CheckDiagnosticContext, CheckExecutionId, CheckSchemaLocation, DependencyGraph, DimensionCheckRound,
    RootedCheckDiagnostic,
};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum CheckRound {
    Default,
    Dimension(DimensionCheckRound),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct CheckRoot {
    pub(crate) execution: StableExecutionId,
    pub(crate) round: CheckRound,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum StableExecutionId {
    Record(RecordCoordinate),
    TopLevel(coflow_cft::CheckName),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LogicalCheckDiagnostic {
    pub(crate) code: coflow_data_model::CfdErrorCode,
    pub(crate) stage: CfdStage,
    pub(crate) severity: CfdSeverity,
    pub(crate) message: String,
    pub(crate) primary: Option<LogicalCheckLabel>,
    pub(crate) related: Vec<LogicalCheckLabel>,
    pub(crate) contexts: Vec<CheckDiagnosticContext>,
    pub(crate) is_custom_message: bool,
    pub(crate) schema_location: Option<CheckSchemaLocation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LogicalCheckLabel {
    pub(crate) record: Option<RecordCoordinate>,
    pub(crate) path: CfdPath,
    pub(crate) message: Option<String>,
    pub(crate) origin: Option<RecordOrigin>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct RootCheckState {
    pub(crate) diagnostics: Vec<LogicalCheckDiagnostic>,
    pub(crate) reads_from: BTreeSet<StableRecordReadDependency>,
    pub(crate) record_sets: BTreeSet<coflow_cft::TypeName>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct StableRecordReadDependency {
    pub(crate) record: RecordCoordinate,
    pub(crate) path: CfdPath,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CheckSnapshot {
    roots: BTreeMap<CheckRoot, RootCheckState>,
    reusable: bool,
}

impl CheckSnapshot {
    pub(crate) fn stable_empty() -> Self {
        Self {
            roots: BTreeMap::new(),
            reusable: true,
        }
    }

    #[must_use]
    pub const fn is_reusable(&self) -> bool {
        self.reusable
    }

    pub(crate) fn insert_execution(
        &mut self,
        model: &CfdDataModel,
        round: &CheckRound,
        targets: &[CheckExecutionId],
        diagnostics: Vec<RootedCheckDiagnostic>,
        dependencies: &DependencyGraph,
    ) -> Option<()> {
        let mut diagnostics_by_root: BTreeMap<CheckExecutionId, Vec<LogicalCheckDiagnostic>> =
            BTreeMap::new();
        for rooted in diagnostics {
            diagnostics_by_root
                .entry(rooted.root)
                .or_default()
                .push(stabilize_diagnostic(model, rooted.diagnostic)?);
        }
        for target in targets {
            let stable = stabilize_execution(model, target)?;
            let reads_from = dependencies
                .reads_from
                .get(target)
                .into_iter()
                .flat_map(|reads| reads.iter())
                .map(|read| {
                    Some(StableRecordReadDependency {
                        record: coordinate_for_id(model, read.record)?,
                        path: read.path.clone(),
                    })
                })
                .collect::<Option<BTreeSet<_>>>()?;
            let record_sets = dependencies
                .record_sets
                .get(target)
                .cloned()
                .unwrap_or_default();
            self.roots.insert(
                CheckRoot {
                    execution: stable,
                    round: round.clone(),
                },
                RootCheckState {
                    diagnostics: diagnostics_by_root.remove(target).unwrap_or_default(),
                    reads_from,
                    record_sets,
                },
            );
        }
        diagnostics_by_root.is_empty().then_some(())
    }

    pub(crate) fn merge_replacement(
        previous: &Self,
        replacement: Self,
        replaced: &BTreeSet<CheckRoot>,
    ) -> Self {
        let mut roots = previous.roots.clone();
        roots.retain(|root, _| !replaced.contains(root));
        roots.extend(replacement.roots);
        Self {
            roots,
            reusable: previous.reusable && replacement.reusable,
        }
    }

    #[must_use]
    pub(crate) fn affected_roots(
        &self,
        changed: &crate::CheckChangeSet,
        rounds: &[DimensionCheckRound],
    ) -> Option<BTreeSet<CheckRoot>> {
        if !self.reusable {
            return None;
        }
        let mut affected = BTreeSet::new();
        for record in changed.records.keys() {
            affected.insert(CheckRoot {
                execution: StableExecutionId::Record(record.clone()),
                round: CheckRound::Default,
            });
            affected.extend(rounds.iter().cloned().map(|round| CheckRoot {
                execution: StableExecutionId::Record(record.clone()),
                round: CheckRound::Dimension(round),
            }));
        }
        affected.extend(
            self.roots
                .iter()
                .filter(|(_, state)| {
                    state
                        .reads_from
                        .iter()
                        .any(|read| {
                            changed
                                .records
                                .get(&read.record)
                                .is_some_and(|paths| changed_paths_overlap(paths, &read.path))
                        })
                        || state
                            .record_sets
                            .iter()
                            .any(|type_name| changed.memberships.contains(type_name))
                })
                .map(|(root, _)| root.clone()),
        );
        Some(affected)
    }

    #[must_use]
    pub fn render_diagnostics(&self, model: &CfdDataModel) -> Option<Vec<CheckDiagnostic>> {
        if !self.reusable {
            return None;
        }
        let mut diagnostics = self
            .roots
            .iter()
            .flat_map(|(root, state)| {
                state
                    .diagnostics
                    .iter()
                    .cloned()
                    .map(move |diagnostic| (root, diagnostic))
            })
            .collect::<Vec<_>>();
        diagnostics.sort_by_key(|(root, _)| {
            match &root.execution {
                StableExecutionId::Record(record) => (0, model
                    .record_by_type_key(&record.actual_type, &record.key)
                    .map_or(usize::MAX, CfdRecordId::index), String::new()),
                StableExecutionId::TopLevel(name) => (1, 0, name.to_string()),
            }
        });
        diagnostics
            .into_iter()
            .map(|(_, diagnostic)| render_diagnostic(model, diagnostic))
            .collect()
    }
}

fn stabilize_diagnostic(
    model: &CfdDataModel,
    diagnostic: CheckDiagnostic,
) -> Option<LogicalCheckDiagnostic> {
    let CheckDiagnostic {
        diagnostic,
        contexts,
        is_custom_message,
        schema_location,
    } = diagnostic;
    let CfdDiagnostic {
        code,
        stage,
        severity,
        message,
        primary,
        related,
    } = diagnostic;
    Some(LogicalCheckDiagnostic {
        code,
        stage,
        severity,
        message,
        primary: match primary {
            Some(label) => Some(stabilize_label(model, label)?),
            None => None,
        },
        related: related
            .into_iter()
            .map(|label| stabilize_label(model, label))
            .collect::<Option<Vec<_>>>()?,
        contexts,
        is_custom_message,
        schema_location,
    })
}

fn stabilize_label(model: &CfdDataModel, label: CfdLabel) -> Option<LogicalCheckLabel> {
    Some(LogicalCheckLabel {
        record: match label.record {
            Some(id) => Some(coordinate_for_id(model, id)?),
            None => None,
        },
        path: label.path,
        message: label.message,
        origin: label.origin,
    })
}

fn render_diagnostic(
    model: &CfdDataModel,
    diagnostic: LogicalCheckDiagnostic,
) -> Option<CheckDiagnostic> {
    Some(CheckDiagnostic {
        diagnostic: CfdDiagnostic {
            code: diagnostic.code,
            stage: diagnostic.stage,
            severity: diagnostic.severity,
            message: diagnostic.message,
            primary: match diagnostic.primary {
                Some(label) => Some(render_label(model, label)?),
                None => None,
            },
            related: diagnostic
                .related
                .into_iter()
                .map(|label| render_label(model, label))
                .collect::<Option<Vec<_>>>()?,
        },
        contexts: diagnostic.contexts,
        is_custom_message: diagnostic.is_custom_message,
        schema_location: diagnostic.schema_location,
    })
}

fn changed_paths_overlap(changed: &crate::ChangedPaths, read: &CfdPath) -> bool {
    match changed {
        crate::ChangedPaths::All => true,
        crate::ChangedPaths::Paths(paths) => paths.iter().any(|changed| {
            path_is_prefix(changed, read) || path_is_prefix(read, changed)
        }),
    }
}

fn path_is_prefix(prefix: &CfdPath, path: &CfdPath) -> bool {
    prefix.segments.len() <= path.segments.len()
        && prefix
            .segments
            .iter()
            .zip(&path.segments)
            .all(|(left, right)| left == right)
}

fn render_label(model: &CfdDataModel, label: LogicalCheckLabel) -> Option<CfdLabel> {
    Some(CfdLabel {
        record: match label.record {
            Some(coordinate) => {
                Some(model.record_by_type_key(&coordinate.actual_type, &coordinate.key)?)
            }
            None => None,
        },
        path: label.path,
        message: label.message,
        origin: label.origin,
    })
}

fn coordinate_for_id(model: &CfdDataModel, id: CfdRecordId) -> Option<RecordCoordinate> {
    model
        .record(id)
        .map(coflow_data_model::CfdRecord::coordinate)
}

fn stabilize_execution(
    model: &CfdDataModel,
    execution: &CheckExecutionId,
) -> Option<StableExecutionId> {
    Some(match execution {
        CheckExecutionId::Record(id) => StableExecutionId::Record(coordinate_for_id(model, *id)?),
        CheckExecutionId::TopLevel(name) => StableExecutionId::TopLevel(name.clone()),
    })
}
