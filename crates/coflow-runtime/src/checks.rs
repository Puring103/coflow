use std::collections::{BTreeMap, BTreeSet};

use coflow_api::{
    byte_range, map_diagnostics_with_origins, DiagnosticContext, DiagnosticSet, Label,
    SourceLocation,
};
use coflow_cft::{
    CftSchema, CheckDependency, CheckField, CheckOwner, CheckStatementId, CheckStatementInfo,
};
use coflow_checker::{
    execute_checks, CheckDiagnostic, CheckDiagnosticContext, CheckExecutionStats, CheckLimits,
    CheckOutput, CheckProjection, CheckTarget, CheckTask,
};
use coflow_data_model::{CfdDataModel, CfdDiagnostics, CfdRecordId, RecordOrigin};

use crate::indexes::DiagnosticLogicalLocation;
use crate::load::logical_locations_from_cfd;
use crate::writes::{ChangedField, ChangedProjection, ChangedRecordFields, CheckImpact};
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
}

impl CheckDiagnosticStore {
    fn replace_results(&mut self, model: &CfdDataModel, output: CheckOutput) {
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

    fn remove_missing_records(&mut self, model: &CfdDataModel) {
        self.by_task.retain(|task, _| match &task.target {
            StableCheckTarget::Record(record) => model
                .record_by_type_key(&record.actual_type, &record.key)
                .is_some(),
            StableCheckTarget::Project => true,
        });
    }

    fn diagnostics(&self, model: &CfdDataModel) -> Vec<CheckDiagnostic> {
        self.by_task
            .values()
            .flatten()
            .filter_map(|stored| restore_diagnostic(model, stored))
            .collect()
    }
}

#[derive(Debug)]
pub(crate) struct ProjectCheckOutput {
    pub(crate) diagnostics: DiagnosticSet,
    pub(crate) logical_locations: BTreeMap<usize, DiagnosticLogicalLocation>,
    pub(crate) state: CheckDiagnosticStore,
    pub(crate) statistics: CheckExecutionStats,
}

pub(crate) fn run_full_project_checks(
    schema: &CftSchema,
    model: &CfdDataModel,
    origins: &[RecordOrigin],
) -> ProjectCheckOutput {
    let limits = CheckLimits::default();
    let tasks = plan_full_checks(schema, model);
    let output = execute_checks(schema, model, tasks, limits);
    let statistics = output.statistics;
    let mut state = CheckDiagnosticStore::default();
    state.replace_results(model, output);
    render_check_store(schema, model, origins, state, statistics)
}

pub(crate) fn run_incremental_project_checks(
    schema: &CftSchema,
    model: &CfdDataModel,
    origins: &[RecordOrigin],
    previous: &CheckDiagnosticStore,
    impact: &CheckImpact,
) -> ProjectCheckOutput {
    let limits = CheckLimits::default();
    let tasks = plan_incremental_checks(schema, model, impact);
    let output = execute_checks(schema, model, tasks, limits);
    let statistics = output.statistics;
    let mut state = previous.clone();
    state.remove_missing_records(model);
    state.replace_results(model, output);
    render_check_store(schema, model, origins, state, statistics)
}

pub(crate) fn plan_full_checks(schema: &CftSchema, model: &CfdDataModel) -> Vec<CheckTask> {
    let mut tasks = BTreeSet::new();
    for (record, value) in model.records() {
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
        insert_task_projections(schema, info, CheckTarget::Project, &mut tasks);
    }
    sorted_tasks(schema, tasks)
}

pub(crate) fn plan_incremental_checks(
    schema: &CftSchema,
    model: &CfdDataModel,
    impact: &CheckImpact,
) -> Vec<CheckTask> {
    let impact = expand_materialization_changes(model, impact);
    let mut tasks = BTreeSet::new();
    for (coordinate, changes) in &impact.records {
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
                    plan_field_change(schema, model, coordinate, record, &field, &mut tasks);
                }
            }
            ChangedRecordFields::Fields(fields) => {
                for field in fields {
                    plan_field_change(schema, model, coordinate, record, field, &mut tasks);
                }
            }
        }
    }

    for changed_type in &impact.record_sets {
        let dependency = CheckDependency::RecordSet(changed_type.clone());
        for statement in schema.check_statements_for_dependency(&dependency) {
            if let Some(info) = schema.check_statement(statement).map(|value| value.info) {
                insert_owner_tasks(
                    schema,
                    model,
                    info,
                    None,
                    &ChangedProjection::Base,
                    &mut tasks,
                );
            }
        }
    }
    sorted_tasks(schema, tasks)
}

fn sorted_tasks(schema: &CftSchema, tasks: BTreeSet<CheckTask>) -> Vec<CheckTask> {
    let mut tasks = tasks.into_iter().collect::<Vec<_>>();
    tasks.sort_by(|left, right| left.execution_cmp(right, schema));
    tasks
}

fn plan_field_change(
    schema: &CftSchema,
    model: &CfdDataModel,
    coordinate: &RecordCoordinate,
    record: Option<CfdRecordId>,
    changed: &ChangedField,
    tasks: &mut BTreeSet<CheckTask>,
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
        insert_owner_tasks(schema, model, info, None, &changed.projection, tasks);
    }
}

fn insert_owner_tasks(
    schema: &CftSchema,
    model: &CfdDataModel,
    info: &CheckStatementInfo,
    direct: Option<CfdRecordId>,
    changed_projection: &ChangedProjection,
    tasks: &mut BTreeSet<CheckTask>,
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
            if let Some(record) = direct {
                insert_changed_projections(
                    schema,
                    info,
                    CheckTarget::Record(record),
                    changed_projection,
                    tasks,
                );
                return;
            }
            let mut records = model
                .records_assignable_to(schema, owner)
                .map(|(id, _)| id)
                .collect::<BTreeSet<_>>();
            for host in schema.check_hosts_for_nested_type(owner) {
                records.extend(model.records_of_type(host).map(|(id, _)| id));
            }
            for record in records {
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
    tasks: &mut BTreeSet<CheckTask>,
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
    tasks: &mut BTreeSet<CheckTask>,
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

fn expand_materialization_changes(model: &CfdDataModel, impact: &CheckImpact) -> CheckImpact {
    let source_ids = impact.records.keys().filter_map(|coordinate| {
        model.record_by_type_key(&coordinate.actual_type, &coordinate.key)
    });
    let mut expanded = impact.clone();
    for id in model.materialization_dependents(source_ids) {
        if let Some(record) = model.record(id) {
            expanded
                .records
                .entry(record.coordinate())
                .or_insert(ChangedRecordFields::All);
        }
    }
    expanded
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

fn render_check_store(
    schema: &CftSchema,
    model: &CfdDataModel,
    origins: &[RecordOrigin],
    state: CheckDiagnosticStore,
    statistics: CheckExecutionStats,
) -> ProjectCheckOutput {
    let raw = state.diagnostics(model);
    let cfd = CfdDiagnostics::new(
        raw.iter()
            .map(|diagnostic| diagnostic.diagnostic.clone())
            .collect(),
    );
    let logical_locations = logical_locations_from_cfd(&cfd, |id| {
        model
            .record(id)
            .map(coflow_data_model::CfdRecord::coordinate)
    });
    ProjectCheckOutput {
        diagnostics: map_check_diagnostics_with_origins(Some(schema), raw, origins),
        logical_locations,
        state,
        statistics,
    }
}

fn map_check_diagnostics_with_origins(
    schema: Option<&CftSchema>,
    diagnostics: Vec<CheckDiagnostic>,
    origins: &[RecordOrigin],
) -> DiagnosticSet {
    let (raw, metadata): (Vec<_>, Vec<_>) = diagnostics
        .into_iter()
        .map(|diagnostic| {
            (
                diagnostic.diagnostic,
                (diagnostic.contexts, diagnostic.schema_location),
            )
        })
        .unzip();
    let mut mapped = map_diagnostics_with_origins(CfdDiagnostics::new(raw), origins);
    for (diagnostic, (contexts, schema_location)) in mapped.diagnostics.iter_mut().zip(metadata) {
        diagnostic.contexts = contexts.into_iter().map(map_check_context).collect();
        if let (Some(schema), Some(location)) = (schema, schema_location) {
            if let Some(source) = schema.source(&location.module) {
                let range = byte_range(&source.source, location.span.start, location.span.end);
                let label = Label {
                    location: SourceLocation::FileSpan {
                        path: source.path.clone(),
                        start_line: range.start.line,
                        start_character: range.start.character,
                        end_line: range.end.line,
                        end_character: range.end.character,
                    },
                    message: Some("check declared here".to_string()),
                };
                if diagnostic.primary.is_none() {
                    diagnostic.primary = Some(label);
                } else {
                    diagnostic.related.push(label);
                }
            }
        }
    }
    mapped
}

fn map_check_context(context: CheckDiagnosticContext) -> DiagnosticContext {
    let mut mapped = DiagnosticContext::default();
    match context {
        CheckDiagnosticContext::Check { name } => {
            mapped.kind = "check".to_string();
            mapped.name = Some(name);
        }
        CheckDiagnosticContext::When { expression } => {
            mapped.kind = "when".to_string();
            mapped.expression = Some(expression);
        }
        CheckDiagnosticContext::Quantifier {
            kind,
            binding,
            item,
        } => {
            mapped.kind = "quantifier".to_string();
            mapped.quantifier = Some(kind);
            mapped.binding = Some(binding);
            mapped.item = Some(item);
        }
        CheckDiagnosticContext::Dimension { dimension, variant } => {
            mapped.kind = "dimension".to_string();
            mapped.dimension = Some(dimension);
            mapped.variant = Some(variant);
        }
    }
    mapped
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::writes::{ChangedField, ChangedProjection, ChangedRecordFields, CheckImpact};
    use coflow_cft::{
        build_schema, parse_modules, CftDimensionInputs, CftFile, DimensionName, FieldName,
        ModuleId, RecordKey, TypeName, VariantName,
    };
    use coflow_data_model::{CfdDataModel, LoadedValueDraft};

    fn schema(source: &str) -> CftSchema {
        let modules = parse_modules([CftFile::from_source(ModuleId::from("main"), source)]);
        build_schema(
            &modules,
            &CftDimensionInputs::try_new([("language", vec!["zh".to_string(), "en".to_string()])])
                .expect("dimensions"),
        )
        .expect("schema")
    }

    fn coordinate(actual_type: &str, key: &str) -> RecordCoordinate {
        RecordCoordinate::new(
            TypeName::new(actual_type).expect("type"),
            RecordKey::new(key).expect("key"),
        )
    }

    fn field(name: &str, projection: ChangedProjection) -> ChangedRecordFields {
        ChangedRecordFields::Fields(BTreeSet::from([ChangedField {
            field: FieldName::new(name).expect("field"),
            projection,
        }]))
    }

    #[test]
    fn direct_field_change_plans_only_the_changed_record_statement() {
        let schema =
            schema("type Item { price: int; name: string; check { price > 0; name != \"\"; } }");
        let mut builder = CfdDataModel::builder(&schema);
        builder.add_record("a", "Item", [("price", 1_i64.into()), ("name", "a".into())]);
        builder.add_record("b", "Item", [("price", 2_i64.into()), ("name", "b".into())]);
        let model = builder.build().expect("model");
        let impact = CheckImpact {
            records: BTreeMap::from([(
                coordinate("Item", "b"),
                field("price", ChangedProjection::Base),
            )]),
            record_sets: BTreeSet::new(),
        };
        let tasks = plan_incremental_checks(&schema, &model, &impact);
        assert_eq!(tasks.len(), 1);
        assert_eq!(
            tasks[0].target,
            CheckTarget::Record(model.record_by_type_key("Item", "b").unwrap())
        );
        assert_eq!(
            schema
                .check_statement(tasks[0].statement)
                .unwrap()
                .info
                .root_index,
            0
        );
    }

    #[test]
    fn cross_type_field_change_fans_out_to_every_owner_record() {
        let schema = schema(
            "type Character { level: int; } type Item { owner: &Character; check { owner.level > 0; } }",
        );
        let mut builder = CfdDataModel::builder(&schema);
        builder.add_record("hero", "Character", [("level", 1_i64.into())]);
        builder.add_record(
            "a",
            "Item",
            [("owner", LoadedValueDraft::record_ref("hero"))],
        );
        builder.add_record(
            "b",
            "Item",
            [("owner", LoadedValueDraft::record_ref("hero"))],
        );
        let model = builder.build().expect("model");
        let impact = CheckImpact {
            records: BTreeMap::from([(
                coordinate("Character", "hero"),
                field("level", ChangedProjection::Base),
            )]),
            record_sets: BTreeSet::new(),
        };
        let tasks = plan_incremental_checks(&schema, &model, &impact);
        assert_eq!(tasks.len(), 2);
        assert_eq!(
            tasks
                .iter()
                .map(|task| task.target)
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([
                CheckTarget::Record(model.record_by_type_key("Item", "a").unwrap()),
                CheckTarget::Record(model.record_by_type_key("Item", "b").unwrap()),
            ])
        );
    }

    #[test]
    fn same_type_record_reference_change_fans_out_to_every_owner_record() {
        let schema = schema(
            r#"
                type Item {
                    value: int;
                    target: &Item? = null;
                    check { target == null || target.value > 0; }
                }
            "#,
        );
        let mut builder = CfdDataModel::builder(&schema);
        builder.add_record(
            "one",
            "Item",
            [("value", 1_i64.into()), ("target", LoadedValueDraft::Null)],
        );
        builder.add_record(
            "two",
            "Item",
            [
                ("value", 2_i64.into()),
                ("target", LoadedValueDraft::record_ref("one")),
            ],
        );
        let model = builder.build().expect("model");
        let impact = CheckImpact {
            records: BTreeMap::from([(
                coordinate("Item", "one"),
                field("value", ChangedProjection::Base),
            )]),
            record_sets: BTreeSet::new(),
        };

        let tasks = plan_incremental_checks(&schema, &model, &impact);

        assert_eq!(tasks.len(), 2);
        assert_eq!(
            tasks
                .iter()
                .map(|task| task.target)
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([
                CheckTarget::Record(model.record_by_type_key("Item", "one").unwrap()),
                CheckTarget::Record(model.record_by_type_key("Item", "two").unwrap()),
            ])
        );
    }

    #[test]
    fn nested_field_change_plans_the_nested_statement_on_only_its_host() {
        let schema = schema(
            r#"
                type Part { value: int; check { value > 0; } }
                type Item { part: Part; }
            "#,
        );
        let build = |a_value: i64| {
            let mut builder = CfdDataModel::builder(&schema);
            builder.add_record(
                "a",
                "Item",
                [(
                    "part",
                    LoadedValueDraft::object("Part", [("value", a_value.into())]),
                )],
            );
            builder.add_record(
                "b",
                "Item",
                [(
                    "part",
                    LoadedValueDraft::object("Part", [("value", 1_i64.into())]),
                )],
            );
            builder.build().expect("model")
        };
        let before = build(-1);
        let previous = full_store(&schema, &before);
        let after = build(1);
        let impact = CheckImpact {
            records: BTreeMap::from([(
                coordinate("Item", "a"),
                field("part", ChangedProjection::Base),
            )]),
            record_sets: BTreeSet::new(),
        };

        let tasks = plan_incremental_checks(&schema, &after, &impact);

        assert_eq!(tasks.len(), 1);
        assert_eq!(
            tasks[0].target,
            CheckTarget::Record(after.record_by_type_key("Item", "a").unwrap())
        );
        assert_eq!(
            schema
                .check_statement(tasks[0].statement)
                .unwrap()
                .info
                .owner,
            CheckOwner::Type(TypeName::new("Part").unwrap())
        );
        assert_eq!(
            incremental_store(&schema, &after, &previous, &impact).diagnostics(&after),
            full_store(&schema, &after).diagnostics(&after)
        );
    }

    #[test]
    fn record_set_change_runs_project_statement_once() {
        let schema =
            schema("type Item { price: int; } check Required { records(Item).len() > 0; }");
        let mut builder = CfdDataModel::builder(&schema);
        builder.add_record("a", "Item", [("price", 1_i64.into())]);
        let model = builder.build().expect("model");
        let impact = CheckImpact {
            records: BTreeMap::from([(coordinate("Item", "a"), ChangedRecordFields::All)]),
            record_sets: BTreeSet::from([TypeName::new("Item").unwrap()]),
        };
        let tasks = plan_incremental_checks(&schema, &model, &impact);
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].target, CheckTarget::Project);
    }

    #[test]
    fn dimension_variant_and_base_changes_plan_the_required_projections() {
        let schema = schema(
            r#"
                type Item {
                    @localized
                    name: string;
                    price: int;
                    check { name.len() <= price; }
                }
            "#,
        );
        let mut builder = CfdDataModel::builder(&schema);
        builder.add_record("a", "Item", [("name", "a".into()), ("price", 2_i64.into())]);
        let model = builder.build().expect("model");
        let variant_impact = CheckImpact {
            records: BTreeMap::from([(
                coordinate("Item", "a"),
                field(
                    "name",
                    ChangedProjection::Dimension {
                        dimension: DimensionName::new("language").unwrap(),
                        variant: VariantName::new("zh").unwrap(),
                    },
                ),
            )]),
            record_sets: BTreeSet::new(),
        };
        let variant_tasks = plan_incremental_checks(&schema, &model, &variant_impact);
        assert_eq!(variant_tasks.len(), 1);
        assert!(matches!(
            &variant_tasks[0].projection,
            CheckProjection::Dimension { dimension, variant }
                if dimension.as_str() == "language" && variant.as_str() == "zh"
        ));

        let base_impact = CheckImpact {
            records: BTreeMap::from([(
                coordinate("Item", "a"),
                field("price", ChangedProjection::Base),
            )]),
            record_sets: BTreeSet::new(),
        };
        let base_tasks = plan_incremental_checks(&schema, &model, &base_impact);
        assert_eq!(base_tasks.len(), 3);
        assert_eq!(base_tasks[0].projection, CheckProjection::Base);
        assert!(matches!(
            &base_tasks[1].projection,
            CheckProjection::Dimension { dimension, variant }
                if dimension.as_str() == "language" && variant.as_str() == "zh"
        ));
        assert!(matches!(
            &base_tasks[2].projection,
            CheckProjection::Dimension { dimension, variant }
                if dimension.as_str() == "language" && variant.as_str() == "en"
        ));
    }

    #[test]
    fn full_plan_runs_project_checks_even_when_the_model_is_empty() {
        let schema = schema("type Item {} check Required { records(Item).len() > 0; }");
        let model = CfdDataModel::builder(&schema).build().expect("model");
        let first = plan_full_checks(&schema, &model);
        let second = plan_full_checks(&schema, &model);
        assert_eq!(first, second);
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].target, CheckTarget::Project);
    }

    fn full_store(schema: &CftSchema, model: &CfdDataModel) -> CheckDiagnosticStore {
        let output = execute_checks(
            schema,
            model,
            plan_full_checks(schema, model),
            CheckLimits::default(),
        );
        let mut state = CheckDiagnosticStore::default();
        state.replace_results(model, output);
        state
    }

    fn incremental_store(
        schema: &CftSchema,
        model: &CfdDataModel,
        previous: &CheckDiagnosticStore,
        impact: &CheckImpact,
    ) -> CheckDiagnosticStore {
        let output = execute_checks(
            schema,
            model,
            plan_incremental_checks(schema, model, impact),
            CheckLimits::default(),
        );
        let mut state = previous.clone();
        state.remove_missing_records(model);
        state.replace_results(model, output);
        state
    }

    #[test]
    fn incremental_scope_replacement_matches_full_and_preserves_unaffected_statements() {
        let schema = schema(
            r#"
                type Item {
                    price: int;
                    name: string;
                    check { price > 0: "price"; name != "": "name"; }
                }
            "#,
        );
        let mut before = CfdDataModel::builder(&schema);
        before.add_record(
            "a",
            "Item",
            [("price", (-1_i64).into()), ("name", "".into())],
        );
        let before = before.build().expect("before");
        let previous = full_store(&schema, &before);

        let mut after = CfdDataModel::builder(&schema);
        after.add_record("a", "Item", [("price", 1_i64.into()), ("name", "".into())]);
        let after = after.build().expect("after");
        let impact = CheckImpact {
            records: BTreeMap::from([(
                coordinate("Item", "a"),
                field("price", ChangedProjection::Base),
            )]),
            record_sets: BTreeSet::new(),
        };
        let incremental = incremental_store(&schema, &after, &previous, &impact);
        let full = full_store(&schema, &after);
        assert_eq!(incremental.diagnostics(&after), full.diagnostics(&after));
        assert_eq!(
            incremental.diagnostics(&after)[0].diagnostic.message,
            "name"
        );
    }

    #[test]
    fn cross_type_incremental_replacement_matches_full_without_runtime_read_edges() {
        let schema = schema(
            "type Character { level: int; } type Item { owner: &Character; check { owner.level > 0; } }",
        );
        let build = |level: i64| {
            let mut builder = CfdDataModel::builder(&schema);
            builder.add_record("hero", "Character", [("level", level.into())]);
            builder.add_record(
                "a",
                "Item",
                [("owner", LoadedValueDraft::record_ref("hero"))],
            );
            builder.add_record(
                "b",
                "Item",
                [("owner", LoadedValueDraft::record_ref("hero"))],
            );
            builder.build().expect("model")
        };
        let before = build(0_i64);
        let previous = full_store(&schema, &before);
        let after = build(1_i64);
        let impact = CheckImpact {
            records: BTreeMap::from([(
                coordinate("Character", "hero"),
                field("level", ChangedProjection::Base),
            )]),
            record_sets: BTreeSet::new(),
        };
        let incremental = incremental_store(&schema, &after, &previous, &impact);
        assert_eq!(
            incremental.diagnostics(&after),
            full_store(&schema, &after).diagnostics(&after)
        );
    }

    #[test]
    fn deleted_record_and_record_set_diagnostics_are_removed_by_stable_scope() {
        let schema =
            schema("type Item {} check Empty { records(Item).len() == 0: \"not empty\"; }");
        let mut before = CfdDataModel::builder(&schema);
        before.add_record("a", "Item", std::iter::empty::<(&str, LoadedValueDraft)>());
        let before = before.build().expect("before");
        let previous = full_store(&schema, &before);
        let after = CfdDataModel::builder(&schema).build().expect("after");
        let impact = CheckImpact {
            records: BTreeMap::from([(coordinate("Item", "a"), ChangedRecordFields::All)]),
            record_sets: BTreeSet::from([TypeName::new("Item").unwrap()]),
        };
        let incremental = incremental_store(&schema, &after, &previous, &impact);
        assert_eq!(
            incremental.diagnostics(&after),
            full_store(&schema, &after).diagnostics(&after)
        );
    }
}
