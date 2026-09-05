use std::collections::BTreeMap;

use coflow_checker::{CheckDiagnostic, CheckOutput, CheckProjection, CheckTarget, CheckTask};
use crate::data_model::{CfdDataModel, CfdRecordId};
use coflow_language::cft::CheckStatementId;

use crate::RecordCoordinate;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum StableCheckTarget {
    Record(RecordCoordinate),
    Project,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct StableCheckTask {
    statement: CheckStatementId,
    target: StableCheckTarget,
    projection: CheckProjection,
}

#[derive(Debug, Clone)]
struct StoredCheckDiagnostic {
    diagnostic: CheckDiagnostic,
    records: BTreeMap<CfdRecordId, RecordCoordinate>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct CheckDiagnosticStore {
    by_task: BTreeMap<StableCheckTask, Vec<StoredCheckDiagnostic>>,
    request_diagnostics: Vec<CheckDiagnostic>,
}

impl CheckDiagnosticStore {
    pub(super) fn replace_results(&mut self, model: &CfdDataModel, output: CheckOutput) {
        self.request_diagnostics = output.request_diagnostics;
        for result in output.results {
            let Some(task) = stable_task(model, &result.task) else {
                continue;
            };
            let diagnostics = result
                .diagnostics
                .into_iter()
                .map(|diagnostic| StoredCheckDiagnostic {
                    records: diagnostic_records(model, &diagnostic),
                    diagnostic,
                })
                .collect();
            self.by_task.insert(task, diagnostics);
        }
    }

    pub(super) fn remove_missing_records(&mut self, model: &CfdDataModel) {
        self.by_task.retain(|task, _| match &task.target {
            StableCheckTarget::Record(record) => model
                .record_by_type_key(&record.actual_type, &record.key)
                .is_some(),
            StableCheckTarget::Project => true,
        });
    }

    pub(super) fn diagnostics(&self, model: &CfdDataModel) -> Vec<CheckDiagnostic> {
        self.request_diagnostics
            .iter()
            .cloned()
            .chain(
                self.by_task
                    .values()
                    .flatten()
                    .filter_map(|stored| restore_diagnostic(model, stored)),
            )
            .collect()
    }
}

fn stable_task(model: &CfdDataModel, task: &CheckTask) -> Option<StableCheckTask> {
    Some(StableCheckTask {
        statement: task.statement,
        target: match task.target {
            CheckTarget::Record(record) => {
                StableCheckTarget::Record(model.record(record)?.coordinate())
            }
            CheckTarget::Project => StableCheckTarget::Project,
        },
        projection: task.projection.clone(),
    })
}

fn diagnostic_records(
    model: &CfdDataModel,
    diagnostic: &CheckDiagnostic,
) -> BTreeMap<CfdRecordId, RecordCoordinate> {
    diagnostic
        .diagnostic
        .primary
        .iter()
        .chain(&diagnostic.diagnostic.related)
        .filter_map(|label| {
            let id = label.record?;
            Some((id, model.record(id)?.coordinate()))
        })
        .collect()
}

fn restore_diagnostic(
    model: &CfdDataModel,
    stored: &StoredCheckDiagnostic,
) -> Option<CheckDiagnostic> {
    let mut diagnostic = stored.diagnostic.clone();
    for label in diagnostic
        .diagnostic
        .primary
        .iter_mut()
        .chain(&mut diagnostic.diagnostic.related)
    {
        let Some(old) = label.record else { continue };
        let coordinate = stored.records.get(&old)?;
        label.record = Some(model.record_by_type_key(&coordinate.actual_type, &coordinate.key)?);
    }
    Some(diagnostic)
}
