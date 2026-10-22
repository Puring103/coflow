use std::collections::BTreeMap;

use crate::data_model::{CfdDataModel, CfdRecordHandle};
use coflow_checker::{CheckDiagnostic, CheckOutput, CheckTarget, CheckTask};
use coflow_language::cft::{CftSchema, CheckStatementId};

use crate::RecordCoordinate;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum StableCheckTarget {
    Record(RecordCoordinate),
    Data {
        record: RecordCoordinate,
        path: coflow_model::CfdPath,
        dimension: Option<(String, String)>,
    },
    Project,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct StableCheckTask {
    statement: CheckStatementId,
    target: StableCheckTarget,
}

#[derive(Debug, Clone)]
struct StoredCheckDiagnostic {
    diagnostic: CheckDiagnostic,
    records: BTreeMap<CfdRecordHandle, RecordCoordinate>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct CheckDiagnosticStore {
    by_task: BTreeMap<StableCheckTask, Vec<StoredCheckDiagnostic>>,
    request_diagnostics: Vec<CheckDiagnostic>,
    variants: BTreeMap<String, Vec<String>>,
    model: Option<CfdDataModel>,
    incomplete: bool,
}

impl CheckDiagnosticStore {
    pub(super) fn replace_results(
        &mut self,
        schema: &CftSchema,
        model: &CfdDataModel,
        output: CheckOutput,
    ) {
        self.model = Some(model.clone());
        self.incomplete = output.diagnostics().any(|diagnostic| {
            diagnostic.diagnostic.code == coflow_model::CfdErrorCode::CheckBudgetExceeded
        });
        self.variants = variant_catalog(schema, model);
        self.request_diagnostics = output.request_diagnostics;
        for result in output.results {
            let Some(task) = stable_task(model, &result.task) else {
                continue;
            };
            let diagnostics: Vec<_> = result
                .diagnostics
                .into_iter()
                .map(|diagnostic| StoredCheckDiagnostic {
                    records: diagnostic_records(model, &diagnostic),
                    diagnostic,
                })
                .collect();
            // 通过的任务无需保留空槽；诊断快照的复制成本与错误数量相关。
            if diagnostics.is_empty() {
                self.by_task.remove(&task);
            } else {
                self.by_task.insert(task, diagnostics);
            }
        }
    }

    pub(super) const fn is_incomplete(&self) -> bool {
        self.incomplete
    }

    pub(super) fn effective_impact(
        &self,
        schema: &CftSchema,
        model: &CfdDataModel,
        impact: &super::impact::CheckImpact,
    ) -> super::impact::CheckImpact {
        self.model.as_ref().map_or_else(
            || impact.clone(),
            |previous| impact.with_model_changes(schema, previous, model),
        )
    }

    pub(super) fn remove_changed_data(&mut self, impact: &super::impact::CheckImpact) {
        self.by_task.retain(|task, _| {
            let StableCheckTarget::Data {
                record,
                path,
                dimension,
            } = &task.target
            else {
                return true;
            };
            let Some(changes) = impact.records.get(record) else {
                return true;
            };
            let (field, variant) = if let Some((field, variant)) = dimension {
                (field.as_str(), Some(variant.as_str()))
            } else if let Some(coflow_model::CfdPathSegment::Field(field)) = path.segments.first() {
                (field.as_str(), None)
            } else {
                return true;
            };
            !changes.affects_owned_field(field, variant)
        });
    }

    // 新增或删除变体会改变所有字段的 variants()，必须重新检查整个数据集合。
    pub(super) fn variant_catalog_changed(&self, schema: &CftSchema, model: &CfdDataModel) -> bool {
        self.variants != variant_catalog(schema, model)
    }

    pub(super) fn remove_missing_records(&mut self, model: &CfdDataModel) {
        self.by_task.retain(|task, _| match &task.target {
            StableCheckTarget::Record(record) | StableCheckTarget::Data { record, .. } => model
                .record_by_type_id(&record.actual_type, &record.id)
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
        target: match &task.target {
            CheckTarget::Record(record) => {
                StableCheckTarget::Record(model.record(*record)?.coordinate())
            }
            CheckTarget::Data {
                record,
                path,
                dimension,
            } => StableCheckTarget::Data {
                record: model.record(*record)?.coordinate(),
                path: path.clone(),
                dimension: dimension.clone(),
            },
            CheckTarget::Project => StableCheckTarget::Project,
        },
    })
}

fn diagnostic_records(
    model: &CfdDataModel,
    diagnostic: &CheckDiagnostic,
) -> BTreeMap<CfdRecordHandle, RecordCoordinate> {
    diagnostic
        .diagnostic
        .primary
        .iter()
        .chain(&diagnostic.diagnostic.related)
        .filter_map(|label| {
            let id = label.record?;
            Some((id, model.record(id)?.coordinate()))
        })
        .chain(diagnostic.value_location.iter().filter_map(|location| {
            Some((location.record, model.record(location.record)?.coordinate()))
        }))
        .collect()
}

fn restore_diagnostic(
    model: &CfdDataModel,
    stored: &StoredCheckDiagnostic,
) -> Option<CheckDiagnostic> {
    let mut diagnostic = stored.diagnostic.clone();
    if let Some(location) = &mut diagnostic.value_location {
        let coordinate = stored.records.get(&location.record)?;
        location.record = model.record_by_type_id(&coordinate.actual_type, &coordinate.id)?;
    }
    for label in diagnostic
        .diagnostic
        .primary
        .iter_mut()
        .chain(&mut diagnostic.diagnostic.related)
    {
        let Some(old) = label.record else { continue };
        let coordinate = stored.records.get(&old)?;
        label.record = Some(model.record_by_type_id(&coordinate.actual_type, &coordinate.id)?);
    }
    Some(diagnostic)
}

fn variant_catalog(schema: &CftSchema, model: &CfdDataModel) -> BTreeMap<String, Vec<String>> {
    schema
        .all_dimensions()
        .map(|dimension| {
            (
                dimension.name.to_string(),
                model
                    .dimension_variants(dimension.name.as_str())
                    .map(ToString::to_string)
                    .collect(),
            )
        })
        .collect()
}
