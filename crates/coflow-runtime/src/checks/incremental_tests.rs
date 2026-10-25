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

#[test]
fn precise_reference_reads_only_recheck_the_actual_inline_owner() {
    let schema = schema("table Config { count: int; } data Reward { target: &Config; check R { target.count > 0; target.count < 10; } } table Item { reward: Reward; }");
    let build = |changed: i64| {
        let mut builder = CfdDataModel::builder(&schema);
        for index in 0..50 {
            builder.add_record(
                format!("c{index}"),
                "Config",
                [("count", if index == 3 { changed } else { 1_i64 }.into())],
            );
        }
        for index in 0..50 {
            builder.add_record(
                format!("i{index}"),
                "Item",
                [(
                    "reward",
                    LoadedValueDraft::object(
                        "Reward",
                        [("target", LoadedValueDraft::record_ref(format!("c{index}")))],
                    ),
                )],
            );
        }
        builder.build().unwrap()
    };
    let before = build(1);
    let previous = run_full_project_checks(&schema, &before, &[]);
    let after = build(-1);
    let output = run_incremental_project_checks(
        &schema,
        &after,
        &[],
        &previous.state,
        &impact("Config", "c3", "count", ChangedProjection::Base),
    );
    assert_eq!(output.statistics.executed_tasks, 2);
    assert_eq!(output.state.diagnostics(&after).len(), 1);
    assert_equivalent(&schema, &after, &output);
    // 共享索引的候选更新不能修改仍被宿主持有的旧代次。
    assert!(previous.state.diagnostics(&before).is_empty());
}

#[test]
fn branch_and_reference_retargeting_replace_the_observed_dependency_set() {
    let schema = schema("table Config { count: int; } table Item { enabled: bool; target: &Config; check R { when enabled { target.count > 0; } } }");
    let build = |enabled: bool, target: &str, a: i64, b: i64| {
        let mut builder = CfdDataModel::builder(&schema);
        builder.add_record("a", "Config", [("count", a.into())]);
        builder.add_record("b", "Config", [("count", b.into())]);
        builder.add_record(
            "item",
            "Item",
            [
                ("enabled", enabled.into()),
                ("target", LoadedValueDraft::record_ref(target)),
            ],
        );
        builder.build().unwrap()
    };
    let before = build(false, "a", 1, -1);
    let mut previous = run_full_project_checks(&schema, &before, &[]).state;
    for (enabled, target, a, b, ty, id, field, executed) in [
        (false, "a", -2, -1, "Config", "a", "count", 0),
        (true, "a", -2, -1, "Item", "item", "enabled", 1),
        (true, "b", -2, -1, "Item", "item", "target", 1),
        (true, "b", 1, -1, "Config", "a", "count", 0),
        (true, "b", 1, 1, "Config", "b", "count", 1),
        (false, "b", 1, 1, "Item", "item", "enabled", 1),
        (false, "b", 1, -3, "Config", "b", "count", 0),
    ] {
        let after = build(enabled, target, a, b);
        let output = run_incremental_project_checks(
            &schema,
            &after,
            &[],
            &previous,
            &impact(ty, id, field, ChangedProjection::Base),
        );
        assert_eq!(output.statistics.executed_tasks, executed);
        assert_equivalent(&schema, &after, &output);
        previous = output.state;
    }
}

#[test]
fn failed_dictionary_lookup_and_rule_abort_recover_from_field_changes() {
    let schema = schema(
        "table Item { entries: {string: int}; check R { entries[\"required\"] > 0; false; } }",
    );
    let build = |present: bool| {
        let mut builder = CfdDataModel::builder(&schema);
        let entries = if present {
            vec![(
                coflow_model::LoadedDictKeyDraft::from("required"),
                1_i64.into(),
            )]
        } else {
            Vec::new()
        };
        builder.add_record(
            "item",
            "Item",
            [("entries", LoadedValueDraft::dict(entries))],
        );
        builder.build().unwrap()
    };
    let before = build(false);
    let previous = run_full_project_checks(&schema, &before, &[]);
    assert_eq!(previous.statistics.executed_tasks, 1);
    let after = build(true);
    let output = run_incremental_project_checks(
        &schema,
        &after,
        &[],
        &previous.state,
        &impact("Item", "item", "entries", ChangedProjection::Base),
    );
    assert_eq!(output.statistics.executed_tasks, 2);
    assert_equivalent(&schema, &after, &output);
    let restored = run_incremental_project_checks(
        &schema,
        &before,
        &[],
        &output.state,
        &impact("Item", "item", "entries", ChangedProjection::Base),
    );
    assert_eq!(restored.statistics.executed_tasks, 1);
    assert_equivalent(&schema, &before, &restored);
}

#[test]
fn empty_record_set_membership_and_later_element_reads_stay_equivalent() {
    let schema = schema("table Item { enabled: bool; } check Available { any item in records(Item) { item.enabled; } }");
    let build = |value: Option<bool>| {
        let mut builder = CfdDataModel::builder(&schema);
        if let Some(value) = value {
            builder.add_record("item", "Item", [("enabled", value.into())]);
        }
        builder.build().unwrap()
    };
    let before = build(None);
    let mut previous = run_full_project_checks(&schema, &before, &[]).state;
    for value in [Some(false), Some(true), None] {
        let after = build(value);
        let output = run_incremental_project_checks(
            &schema,
            &after,
            &[],
            &previous,
            &CheckImpact::default(),
        );
        assert_eq!(output.statistics.executed_tasks, 1);
        assert_equivalent(&schema, &after, &output);
        previous = output.state;
    }
}

#[test]
fn request_and_task_work_interruptions_never_reuse_incomplete_read_graphs() {
    let schema = schema("table Item { nums: [int]; check R { nums.isUnique(); false; } }");
    let mut builder = CfdDataModel::builder(&schema);
    for id in ["a", "b"] {
        builder.add_record(
            id,
            "Item",
            [(
                "nums",
                LoadedValueDraft::Array(vec![1_i64.into(), 2_i64.into()]),
            )],
        );
    }
    let model = builder.build().unwrap();
    for limits in [
        CheckLimits {
            max_request_work: 1,
            ..CheckLimits::default()
        },
        CheckLimits {
            evaluation: coflow_checker::EvaluationLimits::new(1, 100),
            ..CheckLimits::default()
        },
    ] {
        let output = execute_checks(&schema, &model, plan_full_checks(&schema, &model), limits);
        let mut previous = CheckDiagnosticStore::default();
        previous.replace_results(&schema, &model, output);
        assert!(previous.is_incomplete());
        let recovered = run_incremental_project_checks(
            &schema,
            &model,
            &[],
            &previous,
            &CheckImpact::default(),
        );
        assert_eq!(recovered.statistics.executed_tasks, 4);
        assert!(!recovered.state.is_incomplete());
        assert_equivalent(&schema, &model, &recovered);
    }
}

#[test]
fn variant_presence_and_projection_reads_invalidate_independently() {
    let schema = schema(
        r#"table Config { @dimension("mode") text: string; } table Item { target: &Config; check Hard { target.text.hasVariant("hard"); } check Easy { target.text.for("easy") == "base"; } }"#,
    );
    let build = |hard_present: bool, easy: &str| {
        let mut builder = CfdDataModel::builder(&schema);
        for id in ["target", "catalog"] {
            builder.add_record(id, "Config", [("text", "base".into())]);
        }
        builder.add_record(
            "item",
            "Item",
            [("target", LoadedValueDraft::record_ref("target"))],
        );
        builder.add_dimension_value_draft(overlay(
            "Config",
            "catalog",
            "text",
            "hard",
            "base".into(),
        ));
        builder.add_dimension_value_draft(overlay("Config", "target", "text", "easy", easy.into()));
        if hard_present {
            builder.add_dimension_value_draft(overlay(
                "Config",
                "target",
                "text",
                "hard",
                "base".into(),
            ));
        }
        builder.build().unwrap()
    };
    let before = build(false, "base");
    let mut previous = run_full_project_checks(&schema, &before, &[]).state;
    for (hard_present, easy) in [(true, "base"), (true, "different"), (false, "different")] {
        let after = build(hard_present, easy);
        // 覆盖的显式存在状态也是依赖，即使覆盖值等于基础值，hasVariant 仍须重算。
        let output = run_incremental_project_checks(
            &schema,
            &after,
            &[],
            &previous,
            &CheckImpact::default(),
        );
        assert_eq!(output.statistics.executed_tasks, 1);
        assert_equivalent(&schema, &after, &output);
        previous = output.state;
    }
}

#[test]
fn short_circuit_reads_and_custom_message_reads_follow_actual_execution() {
    let schema = schema(
        r#"table Config { count: int; message: string; } table Item { enabled: bool; target: &Config; check R { !enabled || target.count > 0 : f"{target.message}"; } }"#,
    );
    let build = |enabled: bool, count: i64, message: &str| {
        let mut builder = CfdDataModel::builder(&schema);
        builder.add_record(
            "config",
            "Config",
            [("count", count.into()), ("message", message.into())],
        );
        builder.add_record(
            "item",
            "Item",
            [
                ("enabled", enabled.into()),
                ("target", LoadedValueDraft::record_ref("config")),
            ],
        );
        builder.build().unwrap()
    };
    let before = build(false, -1, "first");
    let mut previous = run_full_project_checks(&schema, &before, &[]).state;
    for (enabled, count, message, executed) in [
        (false, -2, "first", 0),
        (true, -2, "first", 1),
        (true, -2, "second", 1),
        (true, 1, "second", 1),
        (true, 1, "third", 0),
    ] {
        let after = build(enabled, count, message);
        let output = run_incremental_project_checks(
            &schema,
            &after,
            &[],
            &previous,
            &CheckImpact::default(),
        );
        assert_eq!(output.statistics.executed_tasks, executed);
        assert_equivalent(&schema, &after, &output);
        previous = output.state;
    }
}

#[test]
fn any_keeps_later_errors_and_later_element_reads_after_a_true_element() {
    let schema = schema(
        r#"table Config { entries: {string: int}; } check R { any item in records(Config) { item.entries["required"] > 0; } }"#,
    );
    let build = |present: bool| {
        let mut builder = CfdDataModel::builder(&schema);
        for id in ["a", "b"] {
            let entries = if id == "a" || present {
                vec![(
                    coflow_model::LoadedDictKeyDraft::from("required"),
                    1_i64.into(),
                )]
            } else {
                Vec::new()
            };
            builder.add_record(id, "Config", [("entries", LoadedValueDraft::dict(entries))]);
        }
        builder.build().unwrap()
    };
    let before = build(false);
    let previous = run_full_project_checks(&schema, &before, &[]);
    assert!(!previous.state.diagnostics(&before).is_empty());
    let after = build(true);
    let output = run_incremental_project_checks(
        &schema,
        &after,
        &[],
        &previous.state,
        &CheckImpact::default(),
    );
    assert_eq!(output.statistics.executed_tasks, 1);
    assert!(output.state.diagnostics(&after).is_empty());
    assert_equivalent(&schema, &after, &output);
}

#[test]
fn deterministic_edit_sequences_match_full_checks_through_references_and_owned_arrays() {
    let schema = schema("table Config { count: int; } data Reward { target: &Config; check R { target.count > 0; target.count < 4; } } table Item { enabled: bool; rewards: [Reward]; check R { !enabled || rewards.len() > 0; } }");
    let mut counts = [1_i64; 6];
    let mut targets = vec![0_usize, 1, 2];
    let mut enabled = true;
    let build = |counts: &[i64; 6], targets: &[usize], enabled: bool| {
        let mut builder = CfdDataModel::builder(&schema);
        for (index, count) in counts.iter().enumerate() {
            builder.add_record(format!("c{index}"), "Config", [("count", (*count).into())]);
        }
        builder.add_record(
            "item",
            "Item",
            [
                ("enabled", enabled.into()),
                (
                    "rewards",
                    LoadedValueDraft::Array(
                        targets
                            .iter()
                            .map(|target| {
                                LoadedValueDraft::object(
                                    "Reward",
                                    [(
                                        "target",
                                        LoadedValueDraft::record_ref(format!("c{target}")),
                                    )],
                                )
                            })
                            .collect(),
                    ),
                ),
            ],
        );
        builder.build().unwrap()
    };
    let before = build(&counts, &targets, enabled);
    let mut previous = run_full_project_checks(&schema, &before, &[]).state;
    let mut seed = 0x5eed_u64;
    for _ in 0..96 {
        seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
        let index = ((seed >> 24) % 6) as usize;
        match seed % 4 {
            0 => counts[index] = ((seed >> 32) % 7) as i64 - 2,
            1 => {
                targets = (0..(seed >> 40) as usize % 5)
                    .map(|offset| (index + offset) % 6)
                    .collect()
            }
            2 => targets.reverse(),
            _ => enabled = !enabled,
        }
        let after = build(&counts, &targets, enabled);
        let output = run_incremental_project_checks(
            &schema,
            &after,
            &[],
            &previous,
            &CheckImpact::default(),
        );
        assert_equivalent(&schema, &after, &output);
        previous = output.state;
    }
}

#[test]
fn signed_zero_updates_custom_diagnostic_text() {
    let schema = schema(r#"table Item { value: float; check R { false : f"{value}"; } }"#);
    let build = |value: f64| {
        let mut builder = CfdDataModel::builder(&schema);
        builder.add_record("item", "Item", [("value", value.into())]);
        builder.build().unwrap()
    };
    let before = build(0.0);
    let previous = run_full_project_checks(&schema, &before, &[]);
    let after = build(-0.0);
    let output = run_incremental_project_checks(
        &schema,
        &after,
        &[],
        &previous.state,
        &CheckImpact::default(),
    );
    assert_eq!(output.statistics.executed_tasks, 1);
    assert_ne!(
        previous.state.diagnostics(&before),
        output.state.diagnostics(&after)
    );
    assert_equivalent(&schema, &after, &output);
}

#[test]
fn unqualified_templates_keep_base_semantics_beside_explicit_dimension_reads() {
    let schema = schema(
        r#"table Text { @dimension("mode") name: string; } table Item { text: string = f"{&Text::source.name}"; @dimension("mode") title: string = f"{self.text}"; check R { text == "base"; title.for("hard") == "good"; } }"#,
    );
    let build = |hard: &str| {
        let mut builder = CfdDataModel::builder(&schema);
        builder.add_record("source", "Text", [("name", "base".into())]);
        builder.add_record(
            "item",
            "Item",
            std::iter::empty::<(&str, LoadedValueDraft)>(),
        );
        builder.add_dimension_value_draft(overlay("Text", "source", "name", "hard", hard.into()));
        builder.build().unwrap()
    };
    let before = build("bad");
    let previous = run_full_project_checks(&schema, &before, &[]);
    let after = build("good");
    let output = run_incremental_project_checks(
        &schema,
        &after,
        &[],
        &previous.state,
        &CheckImpact::default(),
    );
    assert_eq!(output.statistics.executed_tasks, 2);
    assert_equivalent(&schema, &after, &output);
    // 显式投影后的非维度字段成为新的基础值，依然通过普通字段差异使读集失效。
    let dimensions = BTreeMap::from([("mode".to_owned(), "hard".to_owned())]);
    let projected_before = before.project_dimensions(&schema, &dimensions).unwrap();
    let projected_after = after.project_dimensions(&schema, &dimensions).unwrap();
    let previous = run_full_project_checks(&schema, &projected_before, &[]);
    let projected = run_incremental_project_checks(
        &schema,
        &projected_after,
        &[],
        &previous.state,
        &CheckImpact::default(),
    );
    assert_eq!(projected.statistics.executed_tasks, 2);
    assert_equivalent(&schema, &projected_after, &projected);
}

#[test]
fn diagnostic_origin_lookup_matches_explicit_origins_and_preserves_overrides() {
    let schema = schema("table Item { value: int; target: &Item; check R { target.value > 0; } }");
    let origin = |name: &str, line| RecordOrigin::File {
        path: std::path::PathBuf::from(name),
        span: Some(coflow_model::TextSpan {
            start_line: line,
            start_character: 2,
            end_line: line + 2,
            end_character: 3,
        }),
    };
    let mut builder = CfdDataModel::builder(&schema);
    for (id, target, value, line) in [("a", "b", 1_i64, 1), ("b", "a", -1_i64, 10)] {
        builder.add_loaded_record(
            coflow_model::LoadedRecordDraft::new(
                id,
                "Item",
                [
                    ("value", value.into()),
                    ("target", LoadedValueDraft::record_ref(target)),
                ],
            )
            .with_origin(origin(&format!("{id}.cfd"), line)),
        );
    }
    let model = builder.build().unwrap();
    let origins = model
        .records()
        .map(|(_, record)| record.origin.clone())
        .collect::<Vec<_>>();
    let lookup = run_full_project_checks(&schema, &model, &[]);
    let explicit = run_full_project_checks(&schema, &model, &origins);
    assert_eq!(lookup.diagnostics, explicit.diagnostics);
    assert!(!lookup.diagnostics.is_empty());

    let replaced = vec![origin("override-a.cfd", 20), origin("override-b.cfd", 30)];
    let explicit = run_full_project_checks(&schema, &model, &replaced);
    let expected = render::map_check_diagnostics_with_origins(
        Some(&schema),
        lookup.state.diagnostics(&model),
        &replaced,
    );
    assert_eq!(explicit.diagnostics, expected);
    assert_ne!(lookup.diagnostics, explicit.diagnostics);
}
