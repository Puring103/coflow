#![allow(clippy::expect_used, clippy::unwrap_used)]

use super::impact::{ChangedRecordFields, CheckImpact};
use super::*;
use crate::data_model::{CfdDataModel, DimensionValueDraft, LoadedRecordDraft, LoadedValueDraft};
use crate::RecordCoordinate;
use coflow_language::cft::{build_schema, parse_modules, CftFile, ModuleId};

fn schema(source: &str) -> CftSchema {
    build_schema(&parse_modules([CftFile::from_source(
        ModuleId::from("membership"),
        source,
    )]))
    .expect("schema")
}

fn build(schema: &CftSchema, records: &[LoadedRecordDraft]) -> CfdDataModel {
    let mut builder = CfdDataModel::builder(schema);
    for record in records {
        builder.add_loaded_record(record.clone());
    }
    builder.build().unwrap()
}

fn compare_full(
    schema: &CftSchema,
    before: &CfdDataModel,
    previous: &CheckDiagnosticStore,
    after: &CfdDataModel,
    impact: &CheckImpact,
) -> ProjectCheckOutput {
    let old_diagnostics = previous.diagnostics(before);
    let output = run_incremental_project_checks(schema, after, &[], previous, impact);
    // Oracle 独立枚举并执行当前全部规则，比较诊断顺序、位置与逻辑坐标，而非复用增量计划。
    let full = run_full_project_checks(schema, after, &[]);
    assert_eq!(
        output.state.diagnostics(after),
        full.state.diagnostics(after)
    );
    assert_eq!(output.diagnostics, full.diagnostics);
    assert_eq!(output.logical_locations, full.logical_locations);
    assert_eq!(previous.diagnostics(before), old_diagnostics);
    output
}

fn row(id: &str, value: i64, parent: &str) -> LoadedRecordDraft {
    LoadedRecordDraft::new(
        id,
        "Row",
        [
            ("value", value.into()),
            ("parent", LoadedValueDraft::record_ref(parent)),
        ],
    )
}

#[test]
fn member_insert_rename_delete_and_reorder_only_execute_new_roots_and_set_readers() {
    let schema = schema("table Row { value: int; parent: &Row; check R { value > 0; parent.value > 0; } } check Count { records(Row).len() > 0; }");
    let mut records = (0..256)
        .map(|index| row(&format!("r{index}"), 1, "r0"))
        .collect::<Vec<_>>();
    let mut before = build(&schema, &records);
    let mut previous = run_full_project_checks(&schema, &before, &[]).state;
    let original = previous.clone();
    let original_model = before.clone();
    for (operation, expected_tasks) in [("insert", 3), ("rename", 3), ("reorder", 0), ("delete", 1)]
    {
        match operation {
            "insert" => records.push(row("added", -1, "r0")),
            "rename" => records.last_mut().unwrap().id = "renamed".into(),
            "reorder" => records.reverse(),
            _ => records.retain(|record| record.id != "renamed"),
        }
        let after = build(&schema, &records);
        let output = compare_full(&schema, &before, &previous, &after, &CheckImpact::default());
        assert_eq!(
            output.statistics.executed_tasks, expected_tasks,
            "{operation}"
        );
        before = after;
        previous = output.state;
    }
    assert_eq!(original.diagnostics(&original_model), []);
}

#[test]
fn all_fields_only_invalidate_actual_readers_and_always_run_local_constant_roots() {
    let schema = schema(
        "table Row { value: int; parent: &Row; check R { value > 0; parent.value > 0; false; } }",
    );
    let mut records = vec![row("a", 1, "a"), row("b", 1, "a"), row("c", 1, "c")];
    let before = build(&schema, &records);
    let previous = run_full_project_checks(&schema, &before, &[]).state;
    records[0].fields.insert("value".into(), (-1_i64).into());
    let after = build(&schema, &records);
    let impact = CheckImpact {
        records: BTreeMap::from([(
            RecordCoordinate::try_new("Row", "a").unwrap(),
            ChangedRecordFields::All,
        )]),
        ..CheckImpact::default()
    };
    let output = compare_full(&schema, &before, &previous, &after, &impact);
    assert_eq!(output.statistics.executed_tasks, 6);
}

#[test]
fn empty_sets_and_short_circuit_changes_replace_dynamic_collection_dependencies() {
    let schema = schema("singleton Settings { enabled: bool; } table Item { value: int; } check R { !records(Settings)[0].enabled || records(Item).len() > 0; }");
    let make = |enabled: bool, item: bool| {
        let mut records = vec![LoadedRecordDraft::new(
            "Settings",
            "Settings",
            [("enabled", enabled.into())],
        )];
        if item {
            records.push(LoadedRecordDraft::new(
                "item",
                "Item",
                [("value", 1_i64.into())],
            ));
        }
        build(&schema, &records)
    };
    let mut before = make(false, false);
    let mut previous = run_full_project_checks(&schema, &before, &[]).state;
    for (enabled, item, expected) in [
        (false, true, 0),
        (true, true, 1),
        (true, false, 1),
        (true, true, 1),
        (false, true, 1),
        (false, false, 0),
    ] {
        let after = make(enabled, item);
        let output = compare_full(&schema, &before, &previous, &after, &CheckImpact::default());
        assert_eq!(output.statistics.executed_tasks, expected);
        before = after;
        previous = output.state;
    }
}

#[test]
fn reference_type_replacement_rename_and_delete_preserve_inherited_rules_and_virtual_ids() {
    let schema = schema("abstract table Root { value: int; } table Left : Root { check Positive { value > 0; } } table Right : Root {} table Holder { target: &Root; check R { target is Left; target.id == \"target\"; target.value > 0; } } check Counts { records(Root).len() == 1; records(Left).len() == 1; }");
    let make = |actual_type: &str, id: &str, present: bool| {
        let mut records = Vec::new();
        if present {
            records.push(LoadedRecordDraft::new(
                id,
                actual_type,
                [("value", 1_i64.into())],
            ));
            records.push(LoadedRecordDraft::new(
                "holder",
                "Holder",
                [("target", LoadedValueDraft::record_ref(id))],
            ));
        }
        build(&schema, &records)
    };
    let mut before = make("Left", "target", true);
    let mut previous = run_full_project_checks(&schema, &before, &[]).state;
    for (ty, id, present, expected) in [
        ("Right", "target", true, 5),
        ("Left", "renamed", true, 6),
        ("Left", "renamed", false, 2),
    ] {
        let after = make(ty, id, present);
        if ty == "Right" {
            let old = before
                .record(before.record_by_type_id("Holder", "holder").unwrap())
                .unwrap();
            let new = after
                .record(after.record_by_type_id("Holder", "holder").unwrap())
                .unwrap();
            assert_eq!(old.field("target"), new.field("target"));
        }
        let output = compare_full(&schema, &before, &previous, &after, &CheckImpact::default());
        assert_eq!(output.statistics.executed_tasks, expected);
        before = after;
        previous = output.state;
    }
}

fn reward(value: i64, target: &str) -> LoadedValueDraft {
    LoadedValueDraft::Array(vec![LoadedValueDraft::OptionSome(Box::new(
        LoadedValueDraft::object_with_declared_type([
            ("value", value.into()),
            ("target", LoadedValueDraft::record_ref(target)),
        ]),
    ))])
}

#[test]
fn new_nested_and_dimension_instances_run_all_roots_and_removed_paths_disappear() {
    let schema = schema("table Config { value: int; } data Reward { value: int; target: &Config; check R { value > 0; target.value > 0; } } table Item { @dimension(\"mode\") rewards: [Option<Reward>]; check Named { id != \"\"; } } check Count { records(Item).len() > 0; }");
    let make = |ids: &[&str]| {
        let mut builder = CfdDataModel::builder(&schema);
        builder.add_record("config", "Config", [("value", (-1_i64).into())]);
        builder.add_dimension_variant("mode".try_into().unwrap(), "hard".try_into().unwrap());
        for id in ids {
            builder.add_record(*id, "Item", [("rewards", reward(-2, "config"))]);
            builder.add_dimension_value_draft(DimensionValueDraft {
                source_type: "Item".try_into().unwrap(),
                source_id: (*id).try_into().unwrap(),
                field: "rewards".try_into().unwrap(),
                dimension: "mode".try_into().unwrap(),
                variant: "hard".try_into().unwrap(),
                value: reward(-3, "config"),
                origin: RecordOrigin::None,
            });
        }
        builder.build().unwrap()
    };
    let mut before = make(&["a"]);
    let mut previous = run_full_project_checks(&schema, &before, &[]).state;
    for (ids, expected) in [(vec!["a", "b"], 6), (vec!["b", "a"], 0), (vec!["b"], 1)] {
        let after = make(&ids);
        let output = compare_full(&schema, &before, &previous, &after, &CheckImpact::default());
        assert_eq!(output.statistics.executed_tasks, expected);
        before = after;
        previous = output.state;
    }
}

#[test]
fn unchanged_dimension_reference_literal_rechecks_actual_type_replacement() {
    let schema = schema("abstract table Root {} table Left : Root {} table Right : Root {} data Reward { target: &Root; check R { target is Left; target.id != \"\"; } } table Item { @dimension(\"mode\") reward: Reward; }");
    let value = |id: &str| {
        LoadedValueDraft::object_with_declared_type([("target", LoadedValueDraft::record_ref(id))])
    };
    let make = |target_type: &str| {
        let mut builder = CfdDataModel::builder(&schema);
        builder.add_record(
            "target",
            target_type,
            std::iter::empty::<(&str, LoadedValueDraft)>(),
        );
        builder.add_record(
            "other",
            "Left",
            std::iter::empty::<(&str, LoadedValueDraft)>(),
        );
        builder.add_record("item", "Item", [("reward", value("other"))]);
        builder.add_dimension_value_draft(DimensionValueDraft {
            source_type: "Item".try_into().unwrap(),
            source_id: "item".try_into().unwrap(),
            field: "reward".try_into().unwrap(),
            dimension: "mode".try_into().unwrap(),
            variant: "hard".try_into().unwrap(),
            value: value("target"),
            origin: RecordOrigin::None,
        });
        builder.build().unwrap()
    };
    let before = make("Left");
    let previous = run_full_project_checks(&schema, &before, &[]).state;
    let after = make("Right");
    let output = compare_full(&schema, &before, &previous, &after, &CheckImpact::default());
    assert_eq!(output.statistics.executed_tasks, 2);
    assert_eq!(output.state.diagnostics(&after).len(), 1);
}

#[test]
fn skipped_later_root_gains_collection_reads_after_earlier_error_is_repaired() {
    let schema = schema("table Config { entries: {string: int}; } table Item {} check R { records(Config)[0].entries[\"required\"] > 0; records(Item).len() > 0; } check Always { false; false; }");
    let make = |repaired: bool, item: bool| {
        let mut records = vec![LoadedRecordDraft::new(
            "config",
            "Config",
            [(
                "entries",
                LoadedValueDraft::dict(if repaired {
                    vec![(
                        coflow_model::LoadedDictKeyDraft::from("required"),
                        1_i64.into(),
                    )]
                } else {
                    Vec::new()
                }),
            )],
        )];
        if item {
            records.push(LoadedRecordDraft::new(
                "item",
                "Item",
                std::iter::empty::<(&str, LoadedValueDraft)>(),
            ));
        }
        build(&schema, &records)
    };
    let mut before = make(false, false);
    let mut previous = run_full_project_checks(&schema, &before, &[]).state;
    for (repaired, item, expected) in [
        (false, true, 0),
        (true, true, 2),
        (true, false, 2),
        (true, true, 2),
    ] {
        let after = make(repaired, item);
        let output = compare_full(&schema, &before, &previous, &after, &CheckImpact::default());
        assert_eq!(output.statistics.executed_tasks, expected);
        before = after;
        previous = output.state;
    }
}

#[test]
fn rejected_member_plans_and_evaluation_budgets_force_full_recovery() {
    let schema = schema("table Row { value: int; parent: &Row; check R { value > 0; parent.value > 0; } } check Count { records(Row).len() > 0; }");
    let before = build(&schema, &[row("a", -1, "a")]);
    let previous = run_full_project_checks(&schema, &before, &[]).state;
    let after = build(&schema, &[row("a", -1, "a"), row("b", 1, "a")]);
    let impact = previous.effective_impact(&schema, &after, &CheckImpact::default());
    let affected = previous.precise_tasks(&schema, &after, &impact).unwrap();
    let plan = planning::plan_precise_check_plan_with_limit(&schema, &after, &impact, affected, 2);
    assert!(plan.is_err());
    let mut interrupted = previous.clone();
    interrupted.replace_results(
        &schema,
        &after,
        execute_prepared_plan(plan, &after, CheckLimits::default()),
    );
    assert!(interrupted.is_incomplete());
    assert_ne!(interrupted.diagnostics(&after), []);
    let recovered = compare_full(
        &schema,
        &after,
        &interrupted,
        &after,
        &CheckImpact::default(),
    );
    assert_eq!(recovered.statistics.executed_tasks, 5);
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
        let affected = previous.precise_tasks(&schema, &after, &impact).unwrap();
        let plan = planning::plan_precise_check_plan_with_limit(
            &schema,
            &after,
            &impact,
            affected,
            limits.max_tasks,
        );
        let mut interrupted = previous.clone();
        interrupted.replace_results(&schema, &after, execute_prepared_plan(plan, &after, limits));
        assert!(interrupted.is_incomplete());
        let recovered = compare_full(
            &schema,
            &after,
            &interrupted,
            &after,
            &CheckImpact::default(),
        );
        assert_eq!(recovered.statistics.executed_tasks, 5);
        assert!(!recovered.state.is_incomplete());
    }
}

#[test]
fn missing_read_tracking_keeps_static_member_planning() {
    let schema = schema("table Row { value: int; parent: &Row; check R { parent.value > 0; } }");
    let before = build(&schema, &[row("a", 1, "a")]);
    let plan = plan_full_check_plan_with_limit(&schema, &before, usize::MAX);
    let mut output = execute_prepared_plan(plan, &before, CheckLimits::default());
    output.results[0].reads = coflow_checker::CheckReadSet::default();
    let mut previous = CheckDiagnosticStore::default();
    previous.replace_results(&schema, &before, output);
    let after = build(&schema, &[row("a", 1, "a"), row("b", -1, "a")]);
    let impact = previous.effective_impact(&schema, &after, &CheckImpact::default());
    assert!(previous.precise_tasks(&schema, &after, &impact).is_none());
    compare_full(&schema, &before, &previous, &after, &CheckImpact::default());
}

#[test]
fn dangling_reference_host_recovers_after_target_deletion_and_restoration() {
    let schema = schema("table Target { value: int; } table Holder { @dimension(\"mode\") target: &Target; check R { target.value > 0; target.id == \"target\"; } } check Catalog { records(Target).len() == 1; }");
    let make = |value: Option<i64>| {
        let mut builder = CfdDataModel::builder(&schema);
        if let Some(value) = value {
            builder.add_record("target", "Target", [("value", value.into())]);
        }
        builder.add_record(
            "holder",
            "Holder",
            [("target", LoadedValueDraft::record_ref("target"))],
        );
        builder.add_dimension_variant("mode".try_into().unwrap(), "empty".try_into().unwrap());
        let output = builder.build_partial().unwrap();
        assert_eq!(output.diagnostics.is_empty(), value.is_some());
        assert!(output.model.record_by_type_id("Holder", "holder").is_some());
        output.model
    };
    let mut before = make(Some(1));
    let mut previous = run_full_project_checks(&schema, &before, &[]).state;
    let mut snapshots = vec![(
        before.clone(),
        previous.clone(),
        previous.diagnostics(&before),
    )];
    for value in [None, Some(-1), None, Some(1)] {
        let raw = make(value);
        // 空投影保留可修复的模型；未知维度必须继续明确拒绝。
        let after = raw.project_dimensions(&schema, &BTreeMap::new()).unwrap();
        assert!(raw
            .project_dimensions(
                &schema,
                &BTreeMap::from([("unknown_dimension".into(), "empty".into())])
            )
            .is_err());
        let output = compare_full(&schema, &before, &previous, &after, &CheckImpact::default());
        assert_eq!(
            output.statistics.executed_tasks,
            if value.is_some() { 3 } else { 2 }
        );
        if value.is_some() {
            // 空覆盖变体和未知变体都采用基础值，不改变完整/精准检查对照结果。
            for variant in ["empty", "unknown_variant"] {
                let projected = after
                    .project_dimensions(&schema, &BTreeMap::from([("mode".into(), variant.into())]))
                    .unwrap();
                compare_full(
                    &schema,
                    &after,
                    &output.state,
                    &projected,
                    &CheckImpact::default(),
                );
            }
        }
        snapshots.push((
            after.clone(),
            output.state.clone(),
            output.state.diagnostics(&after),
        ));
        before = after;
        previous = output.state;
    }
    assert_eq!(previous.diagnostics(&before), []);
    for (model, state, diagnostics) in snapshots {
        assert_eq!(state.diagnostics(&model), diagnostics);
    }
}
