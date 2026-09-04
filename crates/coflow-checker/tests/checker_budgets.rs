#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

#[path = "checker_common/mod.rs"]
mod common;
use common::*;

const fn limits(max_work: u64, max_iterations: u64) -> EvaluationLimits {
    EvaluationLimits::new(max_work, max_iterations)
}

fn quantified_model() -> (CftSchema, CfdDataModel) {
    let schema = compile_schema(
        r"
            type Item {
                nums: [int];
                check { all number in nums { number > 0; } }
            }
        ",
    );
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "item",
        "Item",
        [(
            "nums",
            LoadedValueDraft::Array(vec![1_i64.into(), 2_i64.into(), 3_i64.into()]),
        )],
    );
    let model = builder.build().expect("model builds");
    (schema, model)
}

#[test]
fn quantifier_iterations_have_an_independent_limit_and_location() {
    let (schema, model) = quantified_model();
    let error = run_model_checks_with_limits(&model, &schema, limits(100, 2))
        .expect_err("third item exceeds the iteration limit");
    let diagnostic = error
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == CfdErrorCode::CheckBudgetExceeded)
        .expect("budget diagnostic");

    assert_eq!(
        diagnostic.message,
        "check evaluation exceeds evaluation iterations limit 2 (observed 3)"
    );
    assert_eq!(
        diagnostic.primary.as_ref().map(|label| label.path.clone()),
        Some(CfdPath::root().field("nums").index(2))
    );
}

#[test]
fn raising_work_does_not_raise_the_iteration_limit() {
    let (schema, model) = quantified_model();
    let error = run_model_checks_with_limits(&model, &schema, limits(u64::MAX, 1))
        .expect_err("iteration budget remains bounded");
    assert_has_code(&error, CfdErrorCode::CheckBudgetExceeded);

    run_model_checks_with_limits(&model, &schema, limits(u64::MAX, 3))
        .expect("the exact iteration boundary succeeds");
}

#[test]
fn aggregate_builtins_charge_work_before_scanning() {
    let schema = compile_schema(
        r"
            type Item {
                nums: [int];
                check { nums.isUnique(); }
            }
        ",
    );
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "item",
        "Item",
        [(
            "nums",
            LoadedValueDraft::Array(vec![1_i64.into(), 2_i64.into(), 3_i64.into(), 4_i64.into()]),
        )],
    );
    let model = builder.build().expect("model builds");

    let error = run_model_checks_with_limits(&model, &schema, limits(3, u64::MAX))
        .expect_err("isUnique charges collection work before scanning");
    let diagnostic = error
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == CfdErrorCode::CheckBudgetExceeded)
        .expect("budget diagnostic");
    assert_eq!(
        diagnostic.message,
        "check evaluation exceeds evaluation work limit 3 (observed 4)"
    );
    assert_eq!(
        diagnostic.primary.as_ref().map(|label| label.path.clone()),
        Some(CfdPath::root().field("nums"))
    );
}

#[test]
fn raising_iterations_does_not_raise_the_work_limit() {
    let schema = compile_schema("type Item { nums: [int]; check { nums.isUnique(); } }");
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "item",
        "Item",
        [("nums", LoadedValueDraft::Array(vec![1_i64.into(), 2_i64.into()]))],
    );
    let model = builder.build().expect("model builds");

    let error = run_model_checks_with_limits(&model, &schema, limits(1, u64::MAX))
        .expect_err("work remains bounded");
    assert_has_code(&error, CfdErrorCode::CheckBudgetExceeded);
    run_model_checks_with_limits(&model, &schema, limits(2, u64::MAX))
        .expect("the exact work boundary succeeds");
}
