pub(crate) mod impact;
#[cfg(test)]
mod incremental_tests;
pub(crate) mod planning;
mod read_graph;
pub(crate) mod render;
mod store;

use std::collections::BTreeMap;

use crate::api::DiagnosticSet;
use crate::data_model::{CfdDataModel, RecordOrigin};
#[cfg(test)]
use coflow_checker::execute_checks;
#[cfg(any(test, feature = "internal-check-bench"))]
use coflow_checker::CheckTask;
use coflow_checker::{CheckExecutionStats, CheckLimits, CheckOutput, CheckPlan};
use coflow_language::cft::CftSchema;

use crate::indexes::DiagnosticLogicalLocation;
use impact::CheckImpact;
#[cfg(any(test, feature = "internal-check-bench"))]
use planning::plan_full_checks_with_limit;
#[cfg(test)]
use planning::plan_incremental_checks_with_limit;
use planning::{
    plan_full_check_plan_with_limit, plan_incremental_check_plan_with_limit, CheckPlanningError,
};
#[cfg(any(test, feature = "internal-check-bench"))]
pub(crate) use planning::{plan_full_checks, plan_incremental_checks};
use render::render_check_store;
pub(crate) use store::CheckDiagnosticStore;

#[cfg(feature = "internal-check-bench")]
pub(crate) fn plan_full_checks_bounded(
    schema: &CftSchema,
    model: &CfdDataModel,
    max_tasks: usize,
) -> Result<Vec<CheckTask>, usize> {
    plan_full_checks_with_limit(schema, model, max_tasks).map_err(|error| error.max_tasks)
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
    let limits = CheckLimits {
        evaluation: crate::limits::RuntimeLimits::default().evaluation,
        ..CheckLimits::default()
    };
    let plan = plan_full_check_plan_with_limit(schema, model, limits.max_tasks);
    let output = execute_prepared_plan(plan, model, limits);
    let statistics = output.statistics;
    let mut state = CheckDiagnosticStore::default();
    state.replace_results(schema, model, output);
    render_check_store(schema, model, origins, state, statistics)
}

pub(crate) fn run_incremental_project_checks(
    schema: &CftSchema,
    model: &CfdDataModel,
    origins: &[RecordOrigin],
    previous: &CheckDiagnosticStore,
    impact: &CheckImpact,
) -> ProjectCheckOutput {
    run_incremental_project_checks_impl(schema, model, origins, previous, impact, None)
}

pub(crate) fn run_incremental_project_checks_with_changed_records(
    schema: &CftSchema,
    model: &CfdDataModel,
    origins: &[RecordOrigin],
    previous: &CheckDiagnosticStore,
    impact: &CheckImpact,
    changed_records: &[crate::data_model::CfdRecordHandle],
) -> ProjectCheckOutput {
    run_incremental_project_checks_impl(
        schema,
        model,
        origins,
        previous,
        impact,
        Some(changed_records),
    )
}

fn run_incremental_project_checks_impl(
    schema: &CftSchema,
    model: &CfdDataModel,
    origins: &[RecordOrigin],
    previous: &CheckDiagnosticStore,
    impact: &CheckImpact,
    changed_records: Option<&[crate::data_model::CfdRecordHandle]>,
) -> ProjectCheckOutput {
    // 预算中断后不把未完成的上一代当作可复用检查结果。
    if previous.is_incomplete() || previous.variant_catalog_changed(schema, model) {
        return run_full_project_checks(schema, model, origins);
    }
    let limits = CheckLimits {
        evaluation: crate::limits::RuntimeLimits::default().evaluation,
        ..CheckLimits::default()
    };
    let impact = changed_records.map_or_else(
        || previous.effective_impact(schema, model, impact),
        |records| previous.effective_impact_for_records(schema, model, impact, records),
    );
    let plan = previous.precise_tasks(schema, model, &impact).map_or_else(
        || plan_incremental_check_plan_with_limit(schema, model, &impact, limits.max_tasks),
        |tasks| {
            planning::plan_precise_check_plan_with_limit(
                schema,
                model,
                &impact,
                tasks,
                limits.max_tasks,
            )
        },
    );
    let planned = plan.is_ok();
    let output = execute_prepared_plan(plan, model, limits);
    let statistics = output.statistics;
    let mut state = previous.clone();
    if !impact.record_sets.is_empty() {
        state.remove_missing_records(model);
    }
    // 规划失败保留既有诊断；成功规划后，清理拥有字段下已消失或移动的内联路径。
    if planned {
        state.remove_changed_data(&impact);
    }
    state.replace_results(schema, model, output);
    render_check_store(schema, model, origins, state, statistics)
}

#[cfg(test)]
fn execute_plan(
    plan: Result<Vec<CheckTask>, CheckPlanningError>,
    schema: &CftSchema,
    model: &CfdDataModel,
    limits: CheckLimits,
) -> CheckOutput {
    execute_prepared_plan(
        plan.map(|tasks| CheckPlan::from_unique_tasks(schema, tasks.into_iter().collect())),
        model,
        limits,
    )
}

fn execute_prepared_plan(
    plan: Result<CheckPlan<'_>, CheckPlanningError>,
    model: &CfdDataModel,
    limits: CheckLimits,
) -> CheckOutput {
    match plan {
        Ok(plan) => plan.execute(model, limits),
        Err(error) => CheckOutput {
            results: Vec::new(),
            request_diagnostics: vec![crate::data_model::CfdDiagnostic::error(
                crate::data_model::CfdErrorCode::CheckBudgetExceeded,
                format!("check task planning limit {} exceeded", error.max_tasks),
            )
            .into()],
            statistics: CheckExecutionStats {
                rejected_tasks: 1,
                ..CheckExecutionStats::default()
            },
        },
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::*;
    use crate::checks::impact::{
        ChangedField, ChangedProjection, ChangedRecordFields, CheckImpact,
    };
    use crate::data_model::{CfdDataModel, DimensionValueDraft, LoadedValueDraft, RecordOrigin};
    use crate::RecordCoordinate;
    use coflow_checker::CheckTarget;
    use coflow_language::cft::{
        build_schema, parse_modules, CftFile, CheckOwner, DimensionName, FieldName, ModuleId,
        RecordId, TypeName, VariantName,
    };
    use std::collections::BTreeSet;

    fn schema(source: &str) -> CftSchema {
        let modules = parse_modules([CftFile::from_source(ModuleId::from("main"), source)]);
        build_schema(&modules).expect("schema")
    }

    fn coordinate(actual_type: &str, key: &str) -> RecordCoordinate {
        RecordCoordinate::new(
            TypeName::new(actual_type).expect("type"),
            RecordId::new(key).expect("key"),
        )
    }

    fn field(name: &str, projection: ChangedProjection) -> ChangedRecordFields {
        ChangedRecordFields::Fields(BTreeSet::from([ChangedField {
            field: FieldName::new(name).expect("field"),
            projection,
        }]))
    }

    #[test]
    fn direct_field_change_plans_the_complete_named_rule_on_the_changed_record() {
        let schema = schema(
            "table Item { price: int; name: string; check Valid { price > 0; name != \"\"; } }",
        );
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
        assert_eq!(tasks.len(), 2);
        assert_eq!(
            tasks[0].target,
            CheckTarget::Record(model.record_by_type_id("Item", "b").unwrap())
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
            "table Character { level: int; } table Item { owner: &Character; check Valid { owner.level > 0; } }",
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
                .map(|task| task.target.clone())
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([
                CheckTarget::Record(model.record_by_type_id("Item", "a").unwrap()),
                CheckTarget::Record(model.record_by_type_id("Item", "b").unwrap()),
            ])
        );
    }

    #[test]
    fn nested_field_change_plans_the_nested_statement_on_only_its_host() {
        let schema = schema(
            r"
                data Part { value: int; }
                table Item { part: Part; check Valid { part.value > 0; } }
            ",
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
            CheckTarget::Record(after.record_by_type_id("Item", "a").unwrap())
        );
        assert_eq!(
            schema
                .check_statement(tasks[0].statement)
                .unwrap()
                .info
                .owner,
            CheckOwner::Type(TypeName::new("Item").unwrap())
        );
        assert_eq!(
            incremental_store(&schema, &after, &previous, &impact).diagnostics(&after),
            full_store(&schema, &after).diagnostics(&after)
        );
    }

    #[test]
    fn record_set_change_runs_project_statement_once() {
        let schema =
            schema("table Item { price: int; } check Required { records(Item).len() > 0; }");
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
    fn dimension_variant_and_base_changes_invalidate_the_same_statement() {
        let schema = schema(
            r#"
                table Item {
                    @localized
                    name: string;
                    price: int;
                    check Valid { name.for("zh").len() <= price; }
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

        let base_impact = CheckImpact {
            records: BTreeMap::from([(
                coordinate("Item", "a"),
                field("price", ChangedProjection::Base),
            )]),
            record_sets: BTreeSet::new(),
        };
        let base_tasks = plan_incremental_checks(&schema, &model, &base_impact);
        assert_eq!(base_tasks.len(), 1);
        assert_eq!(base_tasks, variant_tasks);
    }

    #[test]
    fn full_plan_runs_project_checks_even_when_the_model_is_empty() {
        let schema = schema("table Item {} check Required { records(Item).len() > 0; }");
        let model = CfdDataModel::builder(&schema).build().expect("model");
        let first = plan_full_checks(&schema, &model);
        let second = plan_full_checks(&schema, &model);
        assert_eq!(first, second);
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].target, CheckTarget::Project);
    }

    #[test]
    fn planning_limit_accepts_the_exact_boundary_and_rejects_partial_plans() {
        let schema = schema("table Item { value: int; check Valid { value > 0; } }");
        let mut builder = CfdDataModel::builder(&schema);
        builder.add_record("a", "Item", [("value", 1_i64.into())]);
        builder.add_record("b", "Item", [("value", 1_i64.into())]);
        let model = builder.build().expect("model");

        let exact = plan_full_checks_with_limit(&schema, &model, 2).expect("exact limit");
        assert_eq!(exact.len(), 2);
        assert_eq!(
            plan_full_checks_with_limit(&schema, &model, 1),
            Err(CheckPlanningError { max_tasks: 1 })
        );
    }

    #[test]
    fn planning_failure_preserves_scoped_diagnostics_and_adds_request_diagnostic() {
        let schema = schema("table Item { value: int; check Valid { value > 0; } }");
        let mut builder = CfdDataModel::builder(&schema);
        builder.add_record("a", "Item", [("value", (-1_i64).into())]);
        builder.add_record("b", "Item", [("value", (-1_i64).into())]);
        let model = builder.build().expect("model");
        let previous = full_store(&schema, &model);
        let impact = CheckImpact {
            records: BTreeMap::from([(
                coordinate("Item", "a"),
                field("value", ChangedProjection::Base),
            )]),
            record_sets: BTreeSet::new(),
        };
        let limits = CheckLimits {
            max_tasks: 0,
            ..CheckLimits::default()
        };
        let output = execute_plan(
            plan_incremental_checks_with_limit(&schema, &model, &impact, limits.max_tasks),
            &schema,
            &model,
            limits,
        );
        let mut store = previous;
        store.replace_results(&schema, &model, output);
        let diagnostics = store.diagnostics(&model);

        assert_eq!(diagnostics.len(), 3);
        assert!(diagnostics
            .iter()
            .any(|diagnostic| diagnostic.diagnostic.message.contains("planning limit")));
        assert_eq!(
            diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.diagnostic.message.contains("value > 0"))
                .count(),
            2
        );
    }

    fn full_store(schema: &CftSchema, model: &CfdDataModel) -> CheckDiagnosticStore {
        let output = execute_checks(
            schema,
            model,
            plan_full_checks(schema, model),
            CheckLimits::default(),
        );
        let mut state = CheckDiagnosticStore::default();
        state.replace_results(schema, model, output);
        state
    }

    fn incremental_store(
        schema: &CftSchema,
        model: &CfdDataModel,
        previous: &CheckDiagnosticStore,
        impact: &CheckImpact,
    ) -> CheckDiagnosticStore {
        run_incremental_project_checks(schema, model, &[], previous, impact).state
    }

    #[test]
    fn incremental_scope_replacement_matches_full_and_preserves_unaffected_statements() {
        let schema = schema(
            r#"
                table Item {
                    price: int;
                    name: string;
                    check Valid { price > 0: "price"; name != "": "name"; }
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
            "table Character { level: int; } table Item { owner: &Character; check Valid { owner.level > 0; } }",
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
    fn inherited_statement_incremental_replacement_matches_full() {
        let schema = schema(
            "abstract table Base { value: int; check Valid { value > 0; } } table Item : Base {}",
        );
        let build = |value: i64| {
            let mut builder = CfdDataModel::builder(&schema);
            builder.add_record("a", "Item", [("value", value.into())]);
            builder.build().expect("model")
        };
        let before = build(-1);
        let previous = full_store(&schema, &before);
        let after = build(1);
        let impact = CheckImpact {
            records: BTreeMap::from([(
                coordinate("Item", "a"),
                field("value", ChangedProjection::Base),
            )]),
            record_sets: BTreeSet::new(),
        };

        assert_eq!(
            incremental_store(&schema, &after, &previous, &impact).diagnostics(&after),
            full_store(&schema, &after).diagnostics(&after)
        );
    }

    #[test]
    fn project_collection_field_change_incremental_replacement_matches_full() {
        let schema = schema(
            "table Item { value: int; } check Positive { all item in records(Item) { item.value > 0; } }",
        );
        let build = |value: i64| {
            let mut builder = CfdDataModel::builder(&schema);
            builder.add_record("a", "Item", [("value", value.into())]);
            builder.build().expect("model")
        };
        let before = build(-1);
        let previous = full_store(&schema, &before);
        let after = build(1);
        let impact = CheckImpact {
            records: BTreeMap::from([(
                coordinate("Item", "a"),
                field("value", ChangedProjection::Base),
            )]),
            record_sets: BTreeSet::new(),
        };

        assert_eq!(
            incremental_store(&schema, &after, &previous, &impact).diagnostics(&after),
            full_store(&schema, &after).diagnostics(&after)
        );
    }

    #[test]
    fn dimension_variant_incremental_replacement_matches_full() {
        let schema = schema(
            r#"
                table Item {
                    @localized
                    name: string;
                    check Valid { name.for("zh") != ""; }
                }
            "#,
        );
        let build = |zh: &str| {
            let mut builder = CfdDataModel::builder(&schema);
            builder.add_record("a", "Item", [("name", "base".into())]);
            builder.add_dimension_value_draft(DimensionValueDraft {
                source_type: TypeName::new("Item").unwrap(),
                source_id: RecordId::new("a").unwrap(),
                field: FieldName::new("name").unwrap(),
                dimension: DimensionName::new("language").unwrap(),
                variant: VariantName::new("zh").unwrap(),
                value: zh.into(),
                origin: RecordOrigin::None,
            });
            builder.build().expect("model")
        };
        let before = build("");
        let previous = full_store(&schema, &before);
        let after = build("translated");
        let impact = CheckImpact {
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

        assert_eq!(
            incremental_store(&schema, &after, &previous, &impact).diagnostics(&after),
            full_store(&schema, &after).diagnostics(&after)
        );
    }

    #[test]
    fn deleted_record_and_record_set_diagnostics_are_removed_by_stable_scope() {
        let schema =
            schema("table Item {} check Empty { records(Item).len() == 0: \"not empty\"; }");
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
    #[test]
    fn adding_a_variant_invalidates_other_records_variant_collections() {
        let schema = schema(
            r"table Item { @localized name: string; check Valid { name.variants().len() <= 1; } }",
        );
        let build = |extra: bool| {
            let mut builder = CfdDataModel::builder(&schema);
            builder.add_record("a", "Item", [("name", "a".into())]);
            builder.add_record("b", "Item", [("name", "b".into())]);
            builder.add_dimension_variant(
                DimensionName::new("language").unwrap(),
                VariantName::new("ja").unwrap(),
            );
            if extra {
                builder.add_dimension_variant(
                    DimensionName::new("language").unwrap(),
                    VariantName::new("fr").unwrap(),
                );
            }
            builder.build().expect("model")
        };
        let before = build(false);
        let previous = run_full_project_checks(&schema, &before, &[]);
        assert_eq!(
            previous.state.diagnostics(&before),
            [] as [coflow_checker::CheckDiagnostic; 0]
        );
        let after = build(true);
        let impact = CheckImpact {
            records: BTreeMap::from([(
                coordinate("Item", "a"),
                field(
                    "name",
                    ChangedProjection::Dimension {
                        dimension: DimensionName::new("language").unwrap(),
                        variant: VariantName::new("fr").unwrap(),
                    },
                ),
            )]),
            record_sets: BTreeSet::new(),
        };
        let output = run_incremental_project_checks(&schema, &after, &[], &previous.state, &impact);
        assert_eq!(output.statistics.executed_tasks, 2);
        assert_eq!(output.state.diagnostics(&after).len(), 2);
    }
    #[test]
    fn owned_data_checks_cover_collections_options_dimensions_and_clear_removed_paths() {
        let schema = schema(
            r#"data Reward { count: int; check Positive { count > 0; } }
            table Item { rewards: [Option<Reward>]; values: {string: Reward}; @dimension("mode") reward: Reward; }
        "#,
        );
        let reward =
            |count: i64| LoadedValueDraft::object_with_declared_type([("count", count.into())]);
        let build = |invalid: bool| {
            let mut builder = CfdDataModel::builder(&schema);
            builder.add_record(
                "one",
                "Item",
                [
                    (
                        "rewards",
                        LoadedValueDraft::Array(if invalid {
                            vec![
                                LoadedValueDraft::OptionSome(Box::new(reward(-1))),
                                LoadedValueDraft::OptionNone,
                            ]
                        } else {
                            Vec::new()
                        }),
                    ),
                    (
                        "values",
                        LoadedValueDraft::Dict(vec![(
                            crate::data_model::LoadedDictKeyDraft::String("bad".into()),
                            reward(if invalid { -1 } else { 1 }),
                        )]),
                    ),
                    ("reward", reward(1)),
                ],
            );
            if invalid {
                builder.add_dimension_value_draft(DimensionValueDraft {
                    source_type: TypeName::new("Item").unwrap(),
                    source_id: RecordId::new("one").unwrap(),
                    field: FieldName::new("reward").unwrap(),
                    dimension: DimensionName::new("mode").unwrap(),
                    variant: VariantName::new("hard").unwrap(),
                    value: reward(-1),
                    origin: RecordOrigin::None,
                });
            }
            builder.build().expect("model")
        };
        let before = build(true);
        let previous = full_store(&schema, &before);
        assert_eq!(previous.diagnostics(&before).len(), 3);
        let after = build(false);
        let impact = CheckImpact {
            records: BTreeMap::from([(coordinate("Item", "one"), ChangedRecordFields::All)]),
            record_sets: BTreeSet::new(),
        };
        let updated = incremental_store(&schema, &after, &previous, &impact);
        assert_eq!(
            updated.diagnostics(&after),
            [] as [coflow_checker::CheckDiagnostic; 0]
        );
        assert_eq!(
            updated.diagnostics(&after),
            full_store(&schema, &after).diagnostics(&after)
        );
    }
    #[test]
    fn materialized_data_defaults_are_checked_at_each_owner() {
        let schema = schema("data Reward { count: int = 0; check Positive { count > 0; } } table Item { reward: Reward = {}; maybe: Option<Reward> = None; }");
        let mut builder = CfdDataModel::builder(&schema);
        for id in ["first", "second"] {
            builder.add_record(id, "Item", std::iter::empty::<(&str, LoadedValueDraft)>());
        }
        let model = builder.build().expect("model");
        let tasks = plan_full_checks(&schema, &model);
        assert_eq!(tasks.len(), 2);
        let output = execute_checks(&schema, &model, tasks, CheckLimits::default());
        assert_eq!(output.diagnostics().count(), 2);
        for diagnostic in output.diagnostics() {
            assert_eq!(
                diagnostic.diagnostic.primary.as_ref().unwrap().path,
                crate::data_model::CfdPath::root()
                    .field("reward")
                    .field("count")
            );
        }
    }
}
