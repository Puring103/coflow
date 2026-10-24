#![allow(clippy::expect_used, clippy::unwrap_used)]

use super::impact::{ChangedField, ChangedProjection, ChangedRecordFields};
use super::*;
use crate::data_model::{DimensionValueDraft, LoadedValueDraft, RecordOrigin};
use crate::RecordCoordinate;
use coflow_language::cft::{
    build_schema, parse_modules, CftFile, DimensionName, FieldName, ModuleId, RecordId, TypeName,
    VariantName,
};
use std::collections::BTreeSet;

fn schema(source: &str) -> CftSchema {
    build_schema(&parse_modules([CftFile::from_source(
        ModuleId::from("main"),
        source,
    )]))
    .expect("schema")
}

fn impact(ty: &str, id: &str, field: &str, projection: ChangedProjection) -> CheckImpact {
    CheckImpact {
        records: BTreeMap::from([(
            RecordCoordinate::new(TypeName::new(ty).unwrap(), RecordId::new(id).unwrap()),
            ChangedRecordFields::Fields(BTreeSet::from([ChangedField {
                field: FieldName::new(field).unwrap(),
                projection,
            }])),
        )]),
        record_sets: BTreeSet::new(),
    }
}

fn object(count: i64) -> LoadedValueDraft {
    LoadedValueDraft::object_with_declared_type([("count", count.into())])
}

fn overlay(
    ty: &str,
    id: &str,
    field: &str,
    variant: &str,
    value: LoadedValueDraft,
) -> DimensionValueDraft {
    DimensionValueDraft {
        source_type: TypeName::new(ty).unwrap(),
        source_id: RecordId::new(id).unwrap(),
        field: FieldName::new(field).unwrap(),
        dimension: DimensionName::new("mode").unwrap(),
        variant: VariantName::new(variant).unwrap(),
        value,
        origin: RecordOrigin::None,
    }
}

fn assert_equivalent(schema: &CftSchema, model: &CfdDataModel, output: &ProjectCheckOutput) {
    let full = run_full_project_checks(schema, model, &[]);
    assert_eq!(
        output.state.diagnostics(model),
        full.state.diagnostics(model)
    );
}

#[test]
fn unrelated_scalar_change_preserves_data_failures_without_running_tasks() {
    let schema = schema("data Reward { count: int; check Positive { count > 0; } } table Item { name: string; reward: Reward; }");
    let build = |name: &str| {
        let mut builder = CfdDataModel::builder(&schema);
        for id in ["a", "b", "c"] {
            builder.add_record(id, "Item", [("name", name.into()), ("reward", object(-1))]);
        }
        builder.build().unwrap()
    };
    let before = build("before");
    let previous = run_full_project_checks(&schema, &before, &[]);
    let after = build("after");
    let output = run_incremental_project_checks(
        &schema,
        &after,
        &[],
        &previous.state,
        &impact("Item", "a", "name", ChangedProjection::Base),
    );
    assert_eq!(output.statistics.executed_tasks, 0);
    assert_eq!(output.state.diagnostics(&after).len(), 3);
    assert_equivalent(&schema, &after, &output);
}

#[test]
fn array_insert_delete_move_and_replacement_recheck_only_the_owning_field() {
    let schema = schema("data Reward { count: int; check Positive { count > 0; } } table Item { rewards: [Option<Reward>]; spare: Reward; }");
    let build = |counts: &[Option<i64>]| {
        let mut builder = CfdDataModel::builder(&schema);
        builder.add_record(
            "a",
            "Item",
            [
                (
                    "rewards",
                    LoadedValueDraft::Array(
                        counts
                            .iter()
                            .map(|count| {
                                count.map_or(LoadedValueDraft::OptionNone, |count| {
                                    LoadedValueDraft::OptionSome(Box::new(object(count)))
                                })
                            })
                            .collect(),
                    ),
                ),
                ("spare", object(-9)),
            ],
        );
        builder.add_record(
            "b",
            "Item",
            [
                (
                    "rewards",
                    LoadedValueDraft::Array(vec![LoadedValueDraft::OptionSome(Box::new(object(
                        -8,
                    )))]),
                ),
                ("spare", object(-7)),
            ],
        );
        builder.build().unwrap()
    };
    let before = build(&[Some(-1), Some(2), None, Some(-3)]);
    let previous = run_full_project_checks(&schema, &before, &[]);
    for counts in [
        vec![],
        vec![Some(2)],
        vec![Some(-3), None, Some(2), Some(-1)],
        vec![Some(7), Some(-5)],
    ] {
        let after = build(&counts);
        let output = run_incremental_project_checks(
            &schema,
            &after,
            &[],
            &previous.state,
            &impact("Item", "a", "rewards", ChangedProjection::Base),
        );
        assert_eq!(
            output.statistics.executed_tasks,
            counts.iter().flatten().count()
        );
        assert_equivalent(&schema, &after, &output);
    }
}

#[test]
fn replacing_a_polymorphic_object_removes_previous_rule_diagnostics() {
    let schema = schema("abstract data Part { count: int; check BasePositive { count > 0; } } data First : Part { check Minimum { count > 5; } } data Second : Part {} table Item { part: Part; }");
    let build = |ty: &str, count: i64| {
        let mut builder = CfdDataModel::builder(&schema);
        builder.add_record(
            "a",
            "Item",
            [(
                "part",
                LoadedValueDraft::Object {
                    actual_type: Some(ty.into()),
                    fields: BTreeMap::from([("count".into(), count.into())]),
                },
            )],
        );
        builder.build().unwrap()
    };
    let before = build("First", -1);
    let previous = run_full_project_checks(&schema, &before, &[]);
    assert_eq!(previous.state.diagnostics(&before).len(), 2);
    let after = build("Second", 1);
    let output = run_incremental_project_checks(
        &schema,
        &after,
        &[],
        &previous.state,
        &impact("Item", "a", "part", ChangedProjection::Base),
    );
    assert_eq!(output.statistics.executed_tasks, 1);
    assert_eq!(
        output.state.diagnostics(&after),
        [] as [coflow_checker::CheckDiagnostic; 0]
    );
    assert_equivalent(&schema, &after, &output);
}

#[test]
fn reference_dependency_reruns_only_the_affected_rule_on_all_data_owners() {
    let schema = schema("table Config { count: int; } data Reward { target: &Config; count: int; check External { target.count > 0; target.count < 10; } check Local { count > 0; } } table Item { reward: Reward; }");
    let build = |count: i64| {
        let mut builder = CfdDataModel::builder(&schema);
        builder.add_record("config", "Config", [("count", count.into())]);
        for id in ["a", "b"] {
            builder.add_record(
                id,
                "Item",
                [(
                    "reward",
                    LoadedValueDraft::object_with_declared_type([
                        ("target", LoadedValueDraft::record_ref("config")),
                        ("count", (-1_i64).into()),
                    ]),
                )],
            );
        }
        builder.build().unwrap()
    };
    let before = build(-1);
    let previous = run_full_project_checks(&schema, &before, &[]);
    let after = build(1);
    let output = run_incremental_project_checks(
        &schema,
        &after,
        &[],
        &previous.state,
        &impact("Config", "config", "count", ChangedProjection::Base),
    );
    assert_eq!(output.statistics.executed_tasks, 4);
    assert_eq!(output.state.diagnostics(&after).len(), 2);
    assert_equivalent(&schema, &after, &output);
}

#[test]
fn record_membership_changes_preserve_data_diagnostics_across_handle_reordering() {
    let schema = schema("table Config {} data Reward { check Always { false; } } table Item { reward: Reward; } check Required { records(Config).len() > 0; }");
    let build = |with_config: bool| {
        let mut builder = CfdDataModel::builder(&schema);
        if with_config {
            builder.add_record(
                "config",
                "Config",
                std::iter::empty::<(&str, LoadedValueDraft)>(),
            );
        }
        for id in ["a", "b"] {
            builder.add_record(
                id,
                "Item",
                [(
                    "reward",
                    LoadedValueDraft::object_with_declared_type(std::iter::empty::<(
                        &str,
                        LoadedValueDraft,
                    )>()),
                )],
            );
        }
        builder.build().unwrap()
    };
    let before = build(false);
    let previous = run_full_project_checks(&schema, &before, &[]);
    let after = build(true);
    // 有效代次比较同时补齐记录集合变化，任务身份不受模型内句柄重排影响。
    let output = run_incremental_project_checks(
        &schema,
        &after,
        &[],
        &previous.state,
        &CheckImpact::default(),
    );
    assert_eq!(output.statistics.executed_tasks, 1);
    assert_eq!(output.state.diagnostics(&after).len(), 2);
    assert_equivalent(&schema, &after, &output);
}

#[test]
fn existing_variant_change_only_rechecks_that_owned_variant() {
    let schema = schema("data Reward { count: int; check Positive { count > 0; } } table Item { @dimension(\"mode\") reward: Reward; }");
    let build = |hard: i64| {
        let mut builder = CfdDataModel::builder(&schema);
        for id in ["a", "b"] {
            builder.add_record(id, "Item", [("reward", object(-1))]);
        }
        builder.add_dimension_value_draft(overlay("Item", "a", "reward", "hard", object(hard)));
        builder.add_dimension_value_draft(overlay("Item", "b", "reward", "easy", object(-2)));
        builder.build().unwrap()
    };
    let before = build(-1);
    let previous = run_full_project_checks(&schema, &before, &[]);
    let after = build(1);
    let output = run_incremental_project_checks(
        &schema,
        &after,
        &[],
        &previous.state,
        &impact(
            "Item",
            "a",
            "reward",
            ChangedProjection::Dimension {
                dimension: DimensionName::new("mode").unwrap(),
                variant: VariantName::new("hard").unwrap(),
            },
        ),
    );
    assert_eq!(output.statistics.executed_tasks, 1);
    assert_equivalent(&schema, &after, &output);
}

#[test]
fn template_values_propagate_indirect_record_and_variant_changes_to_data_rules() {
    let schema = schema(
        r#"table Text { @dimension("mode") name: string; } data Label { text: string = f"{&Text::text.name}"; check Present { text != ""; } } table Item { @dimension("mode") label: Label = {}; }"#,
    );
    let build = |base: &str, hard: &str| {
        let mut builder = CfdDataModel::builder(&schema);
        builder.add_record("text", "Text", [("name", base.into())]);
        builder.add_record("a", "Item", std::iter::empty::<(&str, LoadedValueDraft)>());
        builder.add_dimension_value_draft(overlay("Text", "text", "name", "hard", hard.into()));
        builder.build().unwrap()
    };
    let before = build("", "");
    let previous = run_full_project_checks(&schema, &before, &[]);
    let after = build("", "translated");
    let output = run_incremental_project_checks(
        &schema,
        &after,
        &[],
        &previous.state,
        &impact(
            "Text",
            "text",
            "name",
            ChangedProjection::Dimension {
                dimension: DimensionName::new("mode").unwrap(),
                variant: VariantName::new("hard").unwrap(),
            },
        ),
    );
    assert_eq!(output.statistics.executed_tasks, 1);
    assert_eq!(output.state.diagnostics(&after).len(), 1);
    assert_equivalent(&schema, &after, &output);
    let after = build("base", "");
    let output = run_incremental_project_checks(
        &schema,
        &after,
        &[],
        &previous.state,
        &impact("Text", "text", "name", ChangedProjection::Base),
    );
    assert_eq!(output.statistics.executed_tasks, 2);
    assert_equivalent(&schema, &after, &output);
}

#[test]
fn incremental_data_plan_respects_budget_for_changed_field_not_project_size() {
    let schema = schema(
        "data Reward { count: int; check Positive { count > 0; } } table Item { reward: Reward; }",
    );
    let mut builder = CfdDataModel::builder(&schema);
    for index in 0..500 {
        builder.add_record(format!("item{index}"), "Item", [("reward", object(1))]);
    }
    let model = builder.build().unwrap();
    assert_eq!(plan_full_checks(&schema, &model).len(), 500);
    let tasks = plan_incremental_checks_with_limit(
        &schema,
        &model,
        &impact("Item", "item100", "reward", ChangedProjection::Base),
        1,
    )
    .unwrap();
    assert_eq!(tasks.len(), 1);
}

#[test]
fn interrupted_plan_forces_complete_recovery_on_the_next_generation() {
    let schema = schema(
        "data Reward { count: int; check Positive { count > 0; } } table Item { reward: Reward; }",
    );
    let build = |count: i64| {
        let mut builder = CfdDataModel::builder(&schema);
        builder.add_record("a", "Item", [("reward", object(count))]);
        builder.add_record("b", "Item", [("reward", object(-2))]);
        builder.build().unwrap()
    };
    let before = build(-1);
    let mut previous = run_full_project_checks(&schema, &before, &[]).state;
    let after = build(1);
    let interrupted = execute_plan(
        Err(CheckPlanningError { max_tasks: 0 }),
        &schema,
        &after,
        CheckLimits::default(),
    );
    previous.replace_results(&schema, &after, interrupted);
    assert_eq!(previous.diagnostics(&after).len(), 3);
    let recovered =
        run_incremental_project_checks(&schema, &after, &[], &previous, &CheckImpact::default());
    assert_eq!(recovered.statistics.executed_tasks, 2);
    assert_eq!(recovered.state.diagnostics(&after).len(), 1);
    assert_equivalent(&schema, &after, &recovered);
}
