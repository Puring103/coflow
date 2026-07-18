use std::collections::{BTreeMap, BTreeSet};

use coflow_data_model::{
    CfdDataModel, CfdDiagnostic, CfdLabel, CfdPath, CfdRecordId, CfdSeverity, CfdStage,
    RecordCoordinate, RecordOrigin,
};

use crate::{DependencyGraph, DimensionCheckRound, RootedCheckDiagnostic};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum CheckRound {
    Default,
    Dimension(DimensionCheckRound),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct CheckRoot {
    pub(crate) record: RecordCoordinate,
    pub(crate) round: CheckRound,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LogicalCheckDiagnostic {
    pub(crate) code: coflow_data_model::CfdErrorCode,
    pub(crate) stage: CfdStage,
    pub(crate) severity: CfdSeverity,
    pub(crate) message: String,
    pub(crate) primary: Option<LogicalCheckLabel>,
    pub(crate) related: Vec<LogicalCheckLabel>,
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
    pub(crate) reads_from: BTreeSet<RecordCoordinate>,
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
        targets: &[CfdRecordId],
        diagnostics: Vec<RootedCheckDiagnostic>,
        dependencies: &DependencyGraph,
    ) -> Option<()> {
        let mut diagnostics_by_root: BTreeMap<CfdRecordId, Vec<LogicalCheckDiagnostic>> =
            BTreeMap::new();
        for rooted in diagnostics {
            diagnostics_by_root
                .entry(rooted.root)
                .or_default()
                .push(stabilize_diagnostic(model, rooted.diagnostic)?);
        }
        for target in targets {
            let coordinate = coordinate_for_id(model, *target)?;
            let reads_from = dependencies
                .reads_from
                .get(target)
                .into_iter()
                .flat_map(|reads| reads.iter().copied())
                .map(|id| coordinate_for_id(model, id))
                .collect::<Option<BTreeSet<_>>>()?;
            self.roots.insert(
                CheckRoot {
                    record: coordinate,
                    round: round.clone(),
                },
                RootCheckState {
                    diagnostics: diagnostics_by_root.remove(target).unwrap_or_default(),
                    reads_from,
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
        changed: &BTreeSet<RecordCoordinate>,
        rounds: &[DimensionCheckRound],
    ) -> Option<BTreeSet<CheckRoot>> {
        if !self.reusable {
            return None;
        }
        let mut affected = BTreeSet::new();
        for record in changed {
            affected.insert(CheckRoot {
                record: record.clone(),
                round: CheckRound::Default,
            });
            affected.extend(rounds.iter().cloned().map(|round| CheckRoot {
                record: record.clone(),
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
                        .any(|record| changed.contains(record))
                })
                .map(|(root, _)| root.clone()),
        );
        Some(affected)
    }

    #[must_use]
    pub fn render_diagnostics(&self, model: &CfdDataModel) -> Option<Vec<CfdDiagnostic>> {
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
            model
                .record_by_type_key(&root.record.actual_type, &root.record.key)
                .map_or(usize::MAX, CfdRecordId::index)
        });
        diagnostics
            .into_iter()
            .map(|(_, diagnostic)| render_diagnostic(model, diagnostic))
            .collect()
    }
}

fn stabilize_diagnostic(
    model: &CfdDataModel,
    diagnostic: CfdDiagnostic,
) -> Option<LogicalCheckDiagnostic> {
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
) -> Option<CfdDiagnostic> {
    Some(CfdDiagnostic {
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
    })
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
