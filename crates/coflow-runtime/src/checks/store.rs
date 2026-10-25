use std::collections::{BTreeMap, BTreeSet};

use crate::data_model::{CfdDataModel, CfdRecordHandle};
use coflow_checker::{CheckDiagnostic, CheckOutput, CheckTarget, CheckTask};
use coflow_language::cft::{CftSchema, CheckStatementId};

use super::read_graph::{
    CheckReadGraph, StableCheckRule, StableFieldKey, StableFieldRead, StoredRuleReads,
};
use crate::RecordCoordinate;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) enum StableCheckTarget {
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
    reads: CheckReadGraph,
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
        // 先按当前模型句柄合并同一规则的各根，再一次性稳定化坐标；避免每根重复分配类型与 ID。
        let mut rule_reads =
            BTreeMap::<(CheckTarget, &str), BTreeSet<coflow_checker::CheckFieldRead>>::new();
        for result in output.results {
            self.reads.tracked &= result.reads.is_tracked();
            if let Some(statement) = schema.check_statement(result.task.statement) {
                rule_reads
                    .entry((result.task.target.clone(), statement.info.rule_id.as_str()))
                    .or_default()
                    .extend(result.reads.fields);
            } else {
                self.reads.tracked = false;
            }
            if result.diagnostics.is_empty() && self.by_task.is_empty() {
                continue;
            }
            let Some(task) = stable_task(model, &result.task) else {
                self.reads.tracked = false;
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
        for ((target, rule_id), fields) in rule_reads {
            let Some(target) = stable_target(model, &target) else {
                self.reads.tracked = false;
                continue;
            };
            let mut reads = StoredRuleReads::default();
            for read in fields {
                if let Some(record) = model.record(read.record) {
                    reads.fields.insert(StableFieldRead {
                        key: StableFieldKey {
                            record: record.coordinate(),
                            field: read.field,
                        },
                        projection: read.projection,
                    });
                } else {
                    self.reads.tracked = false;
                }
            }
            self.reads.replace_rule(
                StableCheckRule {
                    target,
                    rule_id: rule_id.to_owned(),
                },
                reads,
            );
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

    pub(super) fn effective_impact_for_records(
        &self,
        schema: &CftSchema,
        model: &CfdDataModel,
        impact: &super::impact::CheckImpact,
        changed_records: &[CfdRecordHandle],
    ) -> super::impact::CheckImpact {
        self.model.as_ref().map_or_else(
            || impact.clone(),
            |previous| impact.with_changed_records(schema, previous, model, changed_records),
        )
    }

    pub(super) fn remove_changed_data(&mut self, impact: &super::impact::CheckImpact) {
        self.reads.remove_changed_data(impact);
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
        self.reads.remove_missing_records(model);
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

    pub(super) fn precise_tasks(
        &self,
        schema: &CftSchema,
        model: &CfdDataModel,
        impact: &super::impact::CheckImpact,
    ) -> Option<Vec<CheckTask>> {
        // 成员/未知字段集合改变保留静态扩大失效；普通字段更新只访问实际读取者反向索引。
        if !self.reads.tracked
            || self.model.is_none()
            || !impact.record_sets.is_empty()
            || impact
                .records
                .values()
                .any(|change| matches!(change, super::impact::ChangedRecordFields::All))
        {
            return None;
        }
        let mut tasks = Vec::new();
        for rule in self.reads.affected(impact) {
            let Some(target) = restore_target(model, &rule.target) else {
                continue;
            };
            for statement in schema.check_statements_for_rule(&rule.rule_id) {
                // 拥有路径可能已删除或被另一 data 类型替换，旧实例由新树枚举负责替代。
                if target_matches(schema, model, &target, statement) {
                    tasks.push(CheckTask {
                        target: target.clone(),
                        statement,
                    });
                }
            }
        }
        Some(tasks)
    }
}

fn restore_target(model: &CfdDataModel, target: &StableCheckTarget) -> Option<CheckTarget> {
    Some(match target {
        StableCheckTarget::Project => CheckTarget::Project,
        StableCheckTarget::Record(record) => {
            CheckTarget::Record(model.record_by_type_id(&record.actual_type, &record.id)?)
        }
        StableCheckTarget::Data {
            record,
            path,
            dimension,
        } => CheckTarget::Data {
            record: model.record_by_type_id(&record.actual_type, &record.id)?,
            path: path.clone(),
            dimension: dimension.clone(),
        },
    })
}

fn target_matches(
    schema: &CftSchema,
    model: &CfdDataModel,
    target: &CheckTarget,
    statement: CheckStatementId,
) -> bool {
    let Some(info) = schema.check_statement(statement) else {
        return false;
    };
    let coflow_language::cft::CheckOwner::Type(owner) = &info.info.owner else {
        return matches!(target, CheckTarget::Project);
    };
    match target {
        CheckTarget::Record(record) => model.record(*record).is_some_and(|record| {
            schema.check_owner_applies_to_actual(owner, record.actual_type())
        }),
        CheckTarget::Data {
            record,
            path,
            dimension,
        } => {
            let location = coflow_checker::CheckValueLocation {
                record: *record,
                path: path.clone(),
                dimension: dimension.clone(),
            };
            let Some(mut value) = location.value(model) else {
                return false;
            };
            while let coflow_model::CfdValue::OptionSome(inner) = value {
                value = inner;
            }
            matches!(value, coflow_model::CfdValue::Object(object) if schema.check_owner_applies_to_actual(owner, object.actual_type()))
        }
        CheckTarget::Project => false,
    }
}

fn stable_task(model: &CfdDataModel, task: &CheckTask) -> Option<StableCheckTask> {
    Some(StableCheckTask {
        statement: task.statement,
        target: stable_target(model, &task.target)?,
    })
}

fn stable_target(model: &CfdDataModel, target: &CheckTarget) -> Option<StableCheckTarget> {
    Some(match target {
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
