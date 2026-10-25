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
