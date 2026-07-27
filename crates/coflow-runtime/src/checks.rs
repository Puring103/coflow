pub(crate) mod impact;
mod planning;
mod render;
mod store;

use std::collections::BTreeMap;

use coflow_api::DiagnosticSet;
use coflow_cft::CftSchema;
use coflow_checker::{
    execute_checks, CheckExecutionStats, CheckLimits, CheckOutput, CheckTask,
};
use coflow_data_model::{CfdDataModel, RecordOrigin};

use crate::indexes::DiagnosticLogicalLocation;
use impact::CheckImpact;
#[cfg(any(test, feature = "internal-check-bench"))]
pub(crate) use planning::{plan_full_checks, plan_incremental_checks};
use planning::{
    plan_full_checks_with_limit, plan_incremental_checks_with_limit, CheckPlanningError,
};
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
    let limits = CheckLimits::default();
    let output = execute_plan(
        plan_full_checks_with_limit(schema, model, limits.max_tasks),
        schema,
        model,
        limits,
    );
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
    let output = execute_plan(
        plan_incremental_checks_with_limit(schema, model, impact, limits.max_tasks),
        schema,
        model,
        limits,
    );
    let statistics = output.statistics;
    let mut state = previous.clone();
    state.remove_missing_records(model);
    state.replace_results(model, output);
    render_check_store(schema, model, origins, state, statistics)
}

fn execute_plan(
    plan: Result<Vec<CheckTask>, CheckPlanningError>,
    schema: &CftSchema,
    model: &CfdDataModel,
    limits: CheckLimits,
) -> CheckOutput {
    match plan {
        Ok(tasks) => execute_checks(schema, model, tasks, limits),
        Err(error) => CheckOutput {
            results: Vec::new(),
            request_diagnostics: vec![coflow_data_model::CfdDiagnostic::error(
                coflow_data_model::CfdErrorCode::CheckBudgetExceeded,
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
    use super::*;
    use crate::checks::impact::{
        ChangedField, ChangedProjection, ChangedRecordFields, CheckImpact,
    };
    use crate::RecordCoordinate;
    use coflow_cft::{
        build_schema, parse_modules, CftDimensionInputs, CftFile, CheckOwner, DimensionName,
        FieldName, ModuleId, RecordKey, TypeName, VariantName,
    };
    use coflow_checker::{CheckProjection, CheckTarget};
    use coflow_data_model::{CfdDataModel, DimensionValueDraft, LoadedValueDraft, RecordOrigin};
    use std::collections::BTreeSet;

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

    #[test]
    fn planning_limit_accepts_the_exact_boundary_and_rejects_partial_plans() {
        let schema = schema("type Item { value: int; check { value > 0; } }");
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
        let schema = schema("type Item { value: int; check { value > 0; } }");
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
        store.replace_results(&model, output);
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
    fn inherited_statement_incremental_replacement_matches_full() {
        let schema = schema(
            "abstract type Base { value: int; check { value > 0; } } type Item : Base {}",
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
            "type Item { value: int; } check Positive { all item in records(Item) { item.value > 0; } }",
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
                type Item {
                    @localized
                    name: string;
                    check { name != ""; }
                }
            "#,
        );
        let build = |zh: &str| {
            let mut builder = CfdDataModel::builder(&schema);
            builder.add_record("a", "Item", [("name", "base".into())]);
            builder.add_dimension_value_draft(DimensionValueDraft {
                source_type: TypeName::new("Item").unwrap(),
                source_key: RecordKey::new("a").unwrap(),
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
