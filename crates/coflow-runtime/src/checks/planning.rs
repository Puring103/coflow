use std::collections::{BTreeMap, BTreeSet};

use crate::data_model::{CfdDataModel, CfdRecordHandle};
use coflow_checker::{CheckPlan, CheckTarget, CheckTask};
use coflow_language::cft::{
    CftSchema, CheckDependency, CheckField, CheckOwner, CheckStatementInfo,
};

use super::impact::{ChangedField, ChangedProjection, ChangedRecordFields, CheckImpact};
use crate::RecordCoordinate;

#[cfg(any(test, feature = "internal-check-bench"))]
pub(crate) fn plan_full_checks(schema: &CftSchema, model: &CfdDataModel) -> Vec<CheckTask> {
    plan_full_checks_with_limit(schema, model, usize::MAX).unwrap_or_default()
}
#[cfg(any(test, feature = "internal-check-bench"))]
pub(crate) fn plan_full_checks_with_limit(
    schema: &CftSchema,
    model: &CfdDataModel,
    max_tasks: usize,
) -> Result<Vec<CheckTask>, CheckPlanningError> {
    plan_selected_checks_with_limit(schema, model, max_tasks, None)
}

pub(super) fn plan_full_check_plan_with_limit<'schema>(
    schema: &'schema CftSchema,
    model: &CfdDataModel,
    max_tasks: usize,
) -> Result<CheckPlan<'schema>, CheckPlanningError> {
    plan_selected_check_plan_with_limit(schema, model, max_tasks, None)
}

#[cfg(any(test, feature = "internal-check-bench"))]
pub(crate) fn plan_selected_checks_with_limit(
    schema: &CftSchema,
    model: &CfdDataModel,
    max_tasks: usize,
    selected: Option<&BTreeSet<coflow_language::cft::CheckStatementId>>,
) -> Result<Vec<CheckTask>, CheckPlanningError> {
    plan_selected_check_plan_with_limit(schema, model, max_tasks, selected)
        .map(CheckPlan::into_tasks)
}

pub(crate) fn plan_selected_check_plan_with_limit<'schema>(
    schema: &'schema CftSchema,
    model: &CfdDataModel,
    max_tasks: usize,
    selected: Option<&BTreeSet<coflow_language::cft::CheckStatementId>>,
) -> Result<CheckPlan<'schema>, CheckPlanningError> {
    let mut tasks = CheckTaskBuilder::new(max_tasks);
    for (record, value) in model.records() {
        if tasks.overflowed() {
            break;
        }
        insert_data_tasks(schema, model, record, selected, None, &mut tasks);
        for statement in schema.check_statements_for_actual_type(value.actual_type()) {
            if let Some(info) = schema.check_statement(statement).map(|value| value.info) {
                if selected.is_some_and(|selected| !selected.contains(&info.id)) {
                    continue;
                }
                insert_statement_task(schema, info, &CheckTarget::Record(record), &mut tasks);
            }
        }
    }
    for info in schema
        .all_check_statements()
        .filter(|info| matches!(info.owner, CheckOwner::Project(_)))
    {
        if selected.is_some_and(|selected| !selected.contains(&info.id)) {
            continue;
        }
        if tasks.overflowed() {
            break;
        }
        insert_statement_task(schema, info, &CheckTarget::Project, &mut tasks);
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

#[cfg(any(test, feature = "internal-check-bench"))]
pub(super) fn plan_incremental_checks_with_limit(
    schema: &CftSchema,
    model: &CfdDataModel,
    impact: &CheckImpact,
    max_tasks: usize,
) -> Result<Vec<CheckTask>, CheckPlanningError> {
    plan_incremental_check_plan_with_limit(schema, model, impact, max_tasks)
        .map(CheckPlan::into_tasks)
}

pub(super) fn plan_incremental_check_plan_with_limit<'schema>(
    schema: &'schema CftSchema,
    model: &CfdDataModel,
    impact: &CheckImpact,
    max_tasks: usize,
) -> Result<CheckPlan<'schema>, CheckPlanningError> {
    let mut tasks = CheckTaskBuilder::new(max_tasks);
    for (coordinate, changes) in &impact.records {
        if tasks.overflowed() {
            break;
        }
        let record = model.record_by_type_id(&coordinate.actual_type, &coordinate.id);
        if let Some(record) = record {
            insert_data_tasks(schema, model, record, None, Some(changes), &mut tasks);
            let is_structural = impact.record_sets.iter().any(|changed| {
                schema.is_assignable(&coordinate.actual_type, changed)
                    || schema.is_assignable(changed, &coordinate.actual_type)
            });
            if is_structural {
                for statement in schema.check_statements_for_actual_type(&coordinate.actual_type) {
                    if let Some(info) = schema.check_statement(statement).map(|value| value.info) {
                        insert_statement_task(
                            schema,
                            info,
                            &CheckTarget::Record(record),
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
                    .flat_map(coflow_language::cft::CftType::all_fields)
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
    // 多个字段依赖可命中同一 data 规则；汇总后只扫描一次拥有树。
    if !tasks.data_statements.is_empty() {
        let selected = std::mem::take(&mut tasks.data_statements);
        for (record, _) in model.records() {
            if tasks.overflowed() {
                break;
            }
            insert_data_tasks(schema, model, record, Some(&selected), None, &mut tasks);
        }
    }
    tasks.finish(schema)
}

pub(super) fn plan_precise_check_plan_with_limit<'schema>(
    schema: &'schema CftSchema,
    model: &CfdDataModel,
    impact: &CheckImpact,
    affected: Vec<CheckTask>,
    max_tasks: usize,
) -> Result<CheckPlan<'schema>, CheckPlanningError> {
    let mut tasks = CheckTaskBuilder::new(max_tasks);
    // 新增、替换和重排的拥有子树没有上一代读集，按变化字段枚举其当前全部实例。
    for (coordinate, changes) in &impact.records {
        if tasks.overflowed() {
            break;
        }
        if let Some(record) = model.record_by_type_id(&coordinate.actual_type, &coordinate.id) {
            insert_data_tasks(schema, model, record, None, Some(changes), &mut tasks);
        }
    }
    for task in affected {
        if tasks.overflowed() {
            break;
        }
        tasks.insert(task);
    }
    tasks.finish(schema)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CheckPlanningError {
    pub(super) max_tasks: usize,
}

struct CheckTaskBuilder {
    tasks: BTreeSet<CheckTask>,
    rules: BTreeSet<(CheckTarget, String)>,
    owner_rules: BTreeSet<String>,
    data_statements: BTreeSet<coflow_language::cft::CheckStatementId>,
    max_tasks: usize,
    overflowed: bool,
}

impl CheckTaskBuilder {
    const fn new(max_tasks: usize) -> Self {
        Self {
            tasks: BTreeSet::new(),
            rules: BTreeSet::new(),
            owner_rules: BTreeSet::new(),
            data_statements: BTreeSet::new(),
            max_tasks,
            overflowed: false,
        }
    }

    fn insert(&mut self, task: CheckTask) {
        if self.overflowed {
            return;
        }
        if self.tasks.insert(task) && self.tasks.len() > self.max_tasks {
            self.overflowed = true;
        }
    }

    const fn overflowed(&self) -> bool {
        self.overflowed
    }

    fn finish(self, schema: &CftSchema) -> Result<CheckPlan<'_>, CheckPlanningError> {
        if self.overflowed {
            return Err(CheckPlanningError {
                max_tasks: self.max_tasks,
            });
        }
        Ok(CheckPlan::from_unique_tasks(schema, self.tasks))
    }
}
fn plan_field_change(
    schema: &CftSchema,
    model: &CfdDataModel,
    coordinate: &RecordCoordinate,
    record: Option<CfdRecordHandle>,
    changed: &ChangedField,
    tasks: &mut CheckTaskBuilder,
) {
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
                insert_changed_statement_task(
                    schema,
                    info,
                    &CheckTarget::Record(record),
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
    let data_owner = matches!(&info.owner, CheckOwner::Type(owner)
        if schema.resolve_type(owner).is_some_and(|ty| ty.kind == coflow_language::cft::CftTypeKind::Data));
    if !data_owner && !statement_matches_projection(info, changed_projection) {
        return;
    }
    // 先合并失效规则，再枚举拥有者；多个根或字段命中同一规则不会重复扫描记录集合。
    if !tasks.owner_rules.insert(info.rule_id.clone()) {
        return;
    }
    match &info.owner {
        CheckOwner::Project(_) => {
            insert_changed_statement_task(
                schema,
                info,
                &CheckTarget::Project,
                changed_projection,
                tasks,
            );
        }
        CheckOwner::Type(owner) => {
            if schema
                .resolve_type(owner)
                .is_some_and(|ty| ty.kind == coflow_language::cft::CftTypeKind::Data)
            {
                // data 规则可以经 Ref 读取外部记录；该规则的所有拥有实例都失效。
                tasks
                    .data_statements
                    .extend(schema.check_statements_for_rule(&info.rule_id));
                return;
            }
            for record in model.assignable_records(schema, owner) {
                if tasks.overflowed() {
                    break;
                }
                insert_changed_statement_task(
                    schema,
                    info,
                    &CheckTarget::Record(record),
                    changed_projection,
                    tasks,
                );
            }
        }
    }
}

// 同一条声明式规则只执行一次，维度选择由表达式中的 for() 完成。
fn insert_changed_statement_task(
    schema: &CftSchema,
    info: &CheckStatementInfo,
    target: &CheckTarget,
    changed: &ChangedProjection,
    tasks: &mut CheckTaskBuilder,
) {
    if statement_matches_projection(info, changed) {
        insert_statement_task(schema, info, target, tasks);
    }
}

fn statement_matches_projection(info: &CheckStatementInfo, changed: &ChangedProjection) -> bool {
    matches!(changed, ChangedProjection::Base)
        || matches!(changed, ChangedProjection::Dimension { dimension, .. } if info.dimensions.contains(dimension))
}

fn insert_statement_task(
    schema: &CftSchema,
    info: &CheckStatementInfo,
    target: &CheckTarget,
    tasks: &mut CheckTaskBuilder,
) {
    // 规则实例是失效单位，但每个根仍保留独立任务及求值预算。先去重，避免 R 个根扩展 R 次。
    if !tasks.rules.insert((target.clone(), info.rule_id.clone())) {
        return;
    }
    for statement in schema.check_statements_for_rule(&info.rule_id) {
        tasks.insert(CheckTask {
            statement,
            target: target.clone(),
        });
    }
}

fn owner_fits_host(schema: &CftSchema, owner: &CheckOwner, actual_type: &str) -> bool {
    match owner {
        CheckOwner::Project(_) => false,
        CheckOwner::Type(owner) => schema.check_owner_applies_to_actual(owner, actual_type),
    }
}

// 只遍历拥有关系，不沿 Ref 再次枚举对象，避免循环与重复执行。
fn insert_data_tasks(
    schema: &CftSchema,
    model: &CfdDataModel,
    record: CfdRecordHandle,
    selected: Option<&BTreeSet<coflow_language::cft::CheckStatementId>>,
    changes: Option<&ChangedRecordFields>,
    tasks: &mut CheckTaskBuilder,
) {
    use coflow_model::{CfdPath, CfdValue};
    let Some(root) = model.record(record) else {
        return;
    };
    let mut pending = schema.data_check_fields(root.actual_type())
        .filter(|field| changes.is_none_or(|changes| changes.affects_owned_field(field.name.as_str(), None)))
        .filter_map(|field| root.field(field.name.as_str()).map(|value| (value, CfdPath::root().field(field.name.as_str()), None)))
        .collect::<Vec<_>>();
    for meta in schema.data_check_fields(root.actual_type()) {
        let field = &meta.name;
        let Some(values) = root.dimension_fields.get(field) else { continue };
        for (variant, value) in &values.effective_variants {
            if changes.is_some_and(|changes| {
                !changes.affects_owned_field(field.as_str(), Some(variant.as_str()))
            }) {
                continue;
            }
            pending.push((
                value,
                CfdPath::root(),
                Some((field.to_string(), variant.to_string())),
            ));
        }
    }
    while let Some((value, path, dimension)) = pending.pop() {
        if tasks.overflowed() {
            return;
        }
        match value {
            CfdValue::Object(object) => {
                for statement in schema.check_statements_for_actual_type(object.actual_type()) {
                    if selected.is_some_and(|selection| !selection.contains(&statement)) {
                        continue;
                    }
                    tasks.insert(CheckTask {
                        statement,
                        target: CheckTarget::Data {
                            record,
                            path: path.clone(),
                            dimension: dimension.clone(),
                        },
                    });
                }
                for field in schema.data_check_fields(object.actual_type()) {
                    if let Some(value) = object.field(field.name.as_str()) {
                        pending.push((value, path.clone().field(field.name.as_str()), dimension.clone()));
                    }
                }
            }
            CfdValue::OptionSome(value) => pending.push((value, path, dimension)),
            CfdValue::Array(values) => {
                for (index, value) in values.iter().enumerate() {
                    pending.push((value, path.clone().index(index), dimension.clone()));
                }
            }
            CfdValue::Dict(values) => {
                for (key, value) in values {
                    pending.push((value, path.clone().dict_key_value(key), dimension.clone()));
                }
            }
            _ => {}
        }
    }
}
