use std::collections::{BTreeMap, BTreeSet};

use coflow_cft::{CftSchema, CheckDependency, CheckField, CheckOwner, CheckStatementInfo};
use coflow_checker::{CheckProjection, CheckTarget, CheckTask};
use coflow_data_model::{CfdDataModel, CfdRecordId};

use super::impact::{ChangedField, ChangedProjection, ChangedRecordFields, CheckImpact};
use crate::RecordCoordinate;

#[cfg(any(test, feature = "internal-check-bench"))]
pub(crate) fn plan_full_checks(schema: &CftSchema, model: &CfdDataModel) -> Vec<CheckTask> {
    plan_full_checks_with_limit(schema, model, usize::MAX).unwrap_or_default()
}
pub(super) fn plan_full_checks_with_limit(
    schema: &CftSchema,
    model: &CfdDataModel,
    max_tasks: usize,
) -> Result<Vec<CheckTask>, CheckPlanningError> {
    let mut tasks = CheckTaskBuilder::new(max_tasks);
    for (record, value) in model.records() {
        if tasks.overflowed() {
            break;
        }
        for statement in schema.check_statements_for_actual_type(value.actual_type()) {
            if let Some(info) = schema.check_statement(statement).map(|value| value.info) {
                insert_task_projections(schema, info, CheckTarget::Record(record), &mut tasks);
            }
        }
    }
    for info in schema
        .all_check_statements()
        .filter(|info| matches!(info.owner, CheckOwner::Project(_)))
    {
        if tasks.overflowed() {
            break;
        }
        insert_task_projections(schema, info, CheckTarget::Project, &mut tasks);
    }
    tasks.finish(schema)
}

#[cfg(any(test, feature = "internal-check-bench"))]
pub(crate) fn plan_incremental_checks(
    schema: &CftSchema,
    model: &CfdDataModel,
    impact: &CheckImpact,
) -> Vec<CheckTask> {
    plan_incremental_checks_with_limit(schema, model, impact, usize::MAX).unwrap_or_default()
}

pub(super) fn plan_incremental_checks_with_limit(
    schema: &CftSchema,
    model: &CfdDataModel,
    impact: &CheckImpact,
    max_tasks: usize,
) -> Result<Vec<CheckTask>, CheckPlanningError> {
    let mut tasks = CheckTaskBuilder::new(max_tasks);
    for (coordinate, changes) in &impact.records {
        if tasks.overflowed() {
            break;
        }
        let record = model.record_by_type_key(&coordinate.actual_type, &coordinate.key);
        if let Some(record) = record {
            let is_structural = impact.record_sets.iter().any(|changed| {
                schema.is_assignable(&coordinate.actual_type, changed)
                    || schema.is_assignable(changed, &coordinate.actual_type)
            });
            if is_structural {
                for statement in schema.check_statements_for_actual_type(&coordinate.actual_type) {
                    if let Some(info) = schema.check_statement(statement).map(|value| value.info) {
                        insert_task_projections(
                            schema,
                            info,
                            CheckTarget::Record(record),
                            &mut tasks,
                        );
                    }
                }
            }
        }
        match changes {
            ChangedRecordFields::All => {
                let fields = schema
                    .resolve_type(&coordinate.actual_type)
                    .into_iter()
                    .flat_map(coflow_cft::CftType::all_fields)
                    .map(|field| ChangedField {
                        field: field.name.clone(),
                        projection: ChangedProjection::Base,
                    })
                    .collect::<Vec<_>>();
                for field in fields {
                    if tasks.overflowed() {
                        break;
                    }
                    plan_field_change(schema, model, coordinate, record, &field, &mut tasks);
                }
            }
            ChangedRecordFields::Fields(fields) => {
                for field in fields {
                    if tasks.overflowed() {
                        break;
                    }
                    plan_field_change(schema, model, coordinate, record, field, &mut tasks);
                }
            }
        }
    }

    for changed_type in &impact.record_sets {
        if tasks.overflowed() {
            break;
        }
        let dependency = CheckDependency::RecordSet(changed_type.clone());
        for statement in schema.check_statements_for_dependency(&dependency) {
            if let Some(info) = schema.check_statement(statement).map(|value| value.info) {
                insert_owner_tasks(schema, model, info, &ChangedProjection::Base, &mut tasks);
            }
        }
    }
    tasks.finish(schema)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct CheckPlanningError {
    pub(super) max_tasks: usize,
}

struct CheckTaskBuilder {
    tasks: BTreeSet<CheckTask>,
    max_tasks: usize,
    overflowed: bool,
}

impl CheckTaskBuilder {
    const fn new(max_tasks: usize) -> Self {
        Self {
            tasks: BTreeSet::new(),
            max_tasks,
            overflowed: false,
        }
    }

    fn insert(&mut self, task: CheckTask) {
        if self.overflowed || self.tasks.contains(&task) {
            return;
        }
        if self.tasks.len() >= self.max_tasks {
            self.overflowed = true;
            return;
        }
        self.tasks.insert(task);
    }

    const fn overflowed(&self) -> bool {
        self.overflowed
    }

    fn finish(self, schema: &CftSchema) -> Result<Vec<CheckTask>, CheckPlanningError> {
        if self.overflowed {
            return Err(CheckPlanningError {
                max_tasks: self.max_tasks,
            });
        }
        let mut tasks = self.tasks.into_iter().collect::<Vec<_>>();
        tasks.sort_by(|left, right| left.execution_cmp(right, schema));
        Ok(tasks)
    }
}
fn plan_field_change(
    schema: &CftSchema,
    model: &CfdDataModel,
    coordinate: &RecordCoordinate,
    record: Option<CfdRecordId>,
    changed: &ChangedField,
    tasks: &mut CheckTaskBuilder,
) {
    if let Some(record) = record {
        for statement in
            schema.check_statements_for_nested_field(&coordinate.actual_type, &changed.field)
        {
            if let Some(info) = schema.check_statement(statement).map(|value| value.info) {
                insert_changed_projections(
                    schema,
                    info,
                    CheckTarget::Record(record),
                    &changed.projection,
                    tasks,
                );
            }
        }
    }
    let mut owners = vec![coordinate.actual_type.clone()];
    owners.extend(
        schema
            .ancestor_type_names(&coordinate.actual_type)
            .into_iter()
            .flatten()
            .cloned(),
    );
    let mut statements = BTreeMap::new();
    for owner in owners {
        let dependency = CheckDependency::Field(CheckField {
            owner,
            field: changed.field.clone(),
        });
        for statement in schema.check_statements_for_dependency(&dependency) {
            let is_cross_record = schema.check_dependency_is_cross_record(statement, &dependency);
            statements
                .entry(statement)
                .and_modify(|cross_record| *cross_record |= is_cross_record)
                .or_insert(is_cross_record);
        }
    }
    for (statement, is_cross_record) in statements {
        if tasks.overflowed() {
            break;
        }
        let Some(info) = schema.check_statement(statement).map(|value| value.info) else {
            continue;
        };
        if !is_cross_record && owner_fits_host(schema, &info.owner, &coordinate.actual_type) {
            if let Some(record) = record {
                insert_changed_projections(
                    schema,
                    info,
                    CheckTarget::Record(record),
                    &changed.projection,
                    tasks,
                );
            }
            continue;
        }
        insert_owner_tasks(schema, model, info, &changed.projection, tasks);
    }
}
fn insert_owner_tasks(
    schema: &CftSchema,
    model: &CfdDataModel,
    info: &CheckStatementInfo,
    changed_projection: &ChangedProjection,
    tasks: &mut CheckTaskBuilder,
) {
    match &info.owner {
        CheckOwner::Project(_) => {
            insert_changed_projections(
                schema,
                info,
                CheckTarget::Project,
                changed_projection,
                tasks,
            );
        }
        CheckOwner::Type(owner) => {
            let mut records = model
                .records_assignable_to(schema, owner)
                .map(|(id, _)| id)
                .collect::<BTreeSet<_>>();
            for host in schema.check_hosts_for_nested_type(owner) {
                records.extend(model.records_of_type(host).map(|(id, _)| id));
            }
            for record in records {
                if tasks.overflowed() {
                    break;
                }
                insert_changed_projections(
                    schema,
                    info,
                    CheckTarget::Record(record),
                    changed_projection,
                    tasks,
                );
            }
        }
    }
}

fn insert_changed_projections(
    schema: &CftSchema,
    info: &CheckStatementInfo,
    target: CheckTarget,
    changed: &ChangedProjection,
    tasks: &mut CheckTaskBuilder,
) {
    match changed {
        ChangedProjection::Base => insert_task_projections(schema, info, target, tasks),
        ChangedProjection::Dimension { dimension, variant }
            if info.dimensions.contains(dimension) =>
        {
            tasks.insert(CheckTask {
                statement: info.id,
                target,
                projection: CheckProjection::Dimension {
                    dimension: dimension.clone(),
                    variant: variant.clone(),
                },
            });
        }
        ChangedProjection::Dimension { .. } => {}
    }
}

fn insert_task_projections(
    schema: &CftSchema,
    info: &CheckStatementInfo,
    target: CheckTarget,
    tasks: &mut CheckTaskBuilder,
) {
    tasks.insert(CheckTask {
        statement: info.id,
        target,
        projection: CheckProjection::Base,
    });
    for dimension in &info.dimensions {
        if let Some(meta) = schema.resolve_dimension(dimension) {
            for variant in &meta.variants {
                tasks.insert(CheckTask {
                    statement: info.id,
                    target,
                    projection: CheckProjection::Dimension {
                        dimension: dimension.clone(),
                        variant: variant.clone(),
                    },
                });
            }
        }
    }
}

fn owner_fits_host(schema: &CftSchema, owner: &CheckOwner, actual_type: &str) -> bool {
    match owner {
        CheckOwner::Project(_) => false,
        CheckOwner::Type(owner) => {
            schema.check_owner_applies_to_actual(owner, actual_type)
                || schema
                    .check_hosts_for_nested_type(owner)
                    .any(|host| host.as_str() == actual_type)
        }
    }
}
