#![allow(clippy::expect_used, clippy::unwrap_used)]

use coflow_checker::{
    execute_checks, CheckLimits, CheckPlan, CheckTarget, CheckTask, EvaluationLimits,
};
use coflow_language::cft::{build_schema, parse_modules, CftFile, ModuleId};
use coflow_model::{CfdDataModel, LoadedValueDraft};

#[test]
fn prepared_plan_preserves_order_errors_and_both_budget_boundaries() {
    let schema = build_schema(&parse_modules([CftFile::from_source(
        ModuleId::from("plan"),
        "table Item { nums: [int]; divisor: int; check R { nums.isUnique(); nums.isSorted(); 1 / divisor > 0; false; } check S { true; } }",
    )]))
    .unwrap();
    let mut builder = CfdDataModel::builder(&schema);
    for (id, divisor) in [("a", 0_i64), ("b", 1)] {
        builder.add_record(
            id,
            "Item",
            [
                (
                    "nums",
                    LoadedValueDraft::Array(vec![1_i64.into(), 2_i64.into()]),
                ),
                ("divisor", divisor.into()),
            ],
        );
    }
    let model = builder.build().unwrap();
    let tasks: Vec<_> = model
        .records()
        .flat_map(|(record, _)| {
            schema
                .check_statements_for_actual_type("Item")
                .map(move |statement| CheckTask {
                    target: CheckTarget::Record(record),
                    statement,
                })
        })
        .collect();
    for limits in [
        CheckLimits::default(),
        CheckLimits {
            max_tasks: tasks.len(),
            ..CheckLimits::default()
        },
        CheckLimits {
            max_tasks: tasks.len() - 1,
            ..CheckLimits::default()
        },
        CheckLimits {
            max_request_work: 1,
            ..CheckLimits::default()
        },
        CheckLimits {
            max_request_work: 5,
            ..CheckLimits::default()
        },
        CheckLimits {
            evaluation: EvaluationLimits::new(2, 1),
            ..CheckLimits::default()
        },
    ] {
        let public = execute_checks(
            &schema,
            &model,
            tasks.iter().rev().chain(&tasks).cloned(),
            limits,
        );
        let prepared = CheckPlan::from_unique_tasks(&schema, tasks.iter().cloned().collect())
            .execute(&model, limits);
        assert_eq!(prepared, public, "{limits:?}");
    }
}

#[test]
fn cached_record_sets_preserve_cold_and_warm_budget_results() {
    let schema = build_schema(&parse_modules([CftFile::from_source(ModuleId::from("sets"),
        "table Item {} check Count { records(Item).len() == 20; all item in records(Item) { item.id != \"\"; } }")])).unwrap();
    for work in [1, 15, 30, 100, 1000] {
        let mut builder = CfdDataModel::builder(&schema);
        for index in 0..20 { builder.add_record(format!("i{index}"), "Item", std::iter::empty::<(&str, LoadedValueDraft)>()); }
        let model = builder.build().unwrap();
        let tasks: Vec<_> = schema.all_check_statements().map(|info| CheckTask { target: CheckTarget::Project, statement: info.id }).collect();
        let limits = CheckLimits { max_request_work: work, ..CheckLimits::default() };
        let cold = execute_checks(&schema, &model, tasks.clone(), limits);
        let warm = execute_checks(&schema, &model, tasks, limits);
        assert_eq!(cold, warm, "work budget {work}");
    }
}
