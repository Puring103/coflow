#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

mod common;
use common::*;

use coflow_checker::{run_checks_with_options, CheckOptions, StructuralLimits};

const fn options(max_depth: u64, max_nodes: u64, max_work: u64) -> CheckOptions {
    CheckOptions {
        structural_limits: StructuralLimits::new(max_depth, max_nodes, max_work),
    }
}

#[test]
fn expression_depth_limit_stops_the_current_record_with_a_stable_diagnostic() {
    let schema = compile_schema(
        r"
            type Item {
                enabled: bool;
                check { !!!!!!enabled; }
            }
        ",
    );
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("item", "Item", [("enabled", CfdInputValue::from(true))]);
    let model = builder.build().expect("model builds");

    let err = run_checks_with_options(schema.compiled_schema(), &model, options(3, 100, 100))
        .expect_err("deep expression should exhaust the checker depth budget");
    let diagnostic = err
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == CfdErrorCode::CheckBudgetExceeded)
        .expect("budget diagnostic");

    assert_eq!(
        diagnostic.message,
        "check evaluation exceeds structural depth limit 3 (observed 4)"
    );
    assert_eq!(
        diagnostic.primary.as_ref().map(|label| label.path.clone()),
        Some(CfdPath::root())
    );

    run_checks_with_options(schema.compiled_schema(), &model, options(16, 100, 100))
        .expect("the adjacent larger depth limit accepts the same expression");
}

#[test]
fn nested_data_traversal_uses_the_same_depth_contract() {
    let schema = compile_schema("type Node { child: Node? = null; check { true; } }");
    let child = |value| CfdInputValue::object_with_declared_type([("child", value)]);
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "root",
        "Node",
        [("child", child(child(CfdInputValue::Null)))],
    );
    let model = builder.build().expect("model builds");

    let err = run_checks_with_options(schema.compiled_schema(), &model, options(2, 100, 100))
        .expect_err("nested data should exhaust the traversal depth budget");
    let diagnostic = err
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == CfdErrorCode::CheckBudgetExceeded)
        .expect("budget diagnostic");

    assert_eq!(
        diagnostic.primary.as_ref().map(|label| label.path.clone()),
        Some(CfdPath::root().field("child").field("child"))
    );
    assert_eq!(
        diagnostic.message,
        "data value exceeds structural depth limit 2 (observed 3)"
    );
}

#[test]
fn quantifier_work_limit_points_at_the_first_rejected_item() {
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
            CfdInputValue::Array(vec![
                CfdInputValue::from(1_i64),
                CfdInputValue::from(2_i64),
                CfdInputValue::from(3_i64),
                CfdInputValue::from(4_i64),
            ]),
        )],
    );
    let model = builder.build().expect("model builds");

    let err = run_checks_with_options(schema.compiled_schema(), &model, options(100, 100, 6))
        .expect_err("aggregate expansion plus the third item should exceed work six");
    let diagnostic = err
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == CfdErrorCode::CheckBudgetExceeded)
        .expect("budget diagnostic");

    assert_eq!(
        diagnostic.message,
        "quantifier iteration exceeds structural work limit 6 (observed 7)"
    );
    assert_eq!(
        diagnostic.primary.as_ref().map(|label| label.path.clone()),
        Some(CfdPath::root().field("nums").index(2))
    );
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
            CfdInputValue::Array(vec![
                CfdInputValue::from(1_i64),
                CfdInputValue::from(2_i64),
                CfdInputValue::from(3_i64),
                CfdInputValue::from(4_i64),
            ]),
        )],
    );
    let model = builder.build().expect("model builds");

    let err = run_checks_with_options(schema.compiled_schema(), &model, options(100, 100, 3))
        .expect_err("isUnique should charge collection work before scanning");
    let diagnostic = err
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == CfdErrorCode::CheckBudgetExceeded)
        .expect("budget diagnostic");

    assert_eq!(
        diagnostic.primary.as_ref().map(|label| label.path.clone()),
        Some(CfdPath::root().field("nums"))
    );
    assert_eq!(
        diagnostic.message,
        "check evaluation exceeds structural work limit 3 (observed 4)"
    );
}

#[test]
fn aggregate_conversion_charges_nodes_before_copying_items() {
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
            CfdInputValue::Array(vec![
                CfdInputValue::from(1_i64),
                CfdInputValue::from(2_i64),
                CfdInputValue::from(3_i64),
            ]),
        )],
    );
    let model = builder.build().expect("model builds");

    let err = run_checks_with_options(schema.compiled_schema(), &model, options(100, 3, 100))
        .expect_err("the first array item should exceed the conversion node budget");
    let diagnostic = err
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == CfdErrorCode::CheckBudgetExceeded)
        .expect("budget diagnostic");

    assert_eq!(
        diagnostic.message,
        "data value exceeds structural nodes limit 3 (observed 4)"
    );
    assert_eq!(
        diagnostic.primary.as_ref().map(|label| label.path.clone()),
        Some(CfdPath::root().field("nums").index(0))
    );
}

#[test]
fn aggregate_len_keeps_a_borrowed_cursor_without_materializing_items() {
    const ITEM_COUNT_I64: i64 = 100_000;
    let schema = compile_schema(
        r"
            type Item {
                nums: [int];
                check { nums.len() == 100000; }
            }
        ",
    );
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "item",
        "Item",
        [(
            "nums",
            CfdInputValue::Array((0..ITEM_COUNT_I64).map(CfdInputValue::from).collect()),
        )],
    );
    let model = builder.build().expect("model builds");

    run_checks_with_options(schema.compiled_schema(), &model, options(100, 16, 100))
        .expect("len should retain only the aggregate cursor and never visit its elements");
}
