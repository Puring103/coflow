#![allow(clippy::expect_used, clippy::panic_in_result_fn)]

mod common;
use common::*;

const fn limits(max_depth: u64, max_nodes: u64, max_work: u64) -> StructuralLimits {
    StructuralLimits::new(max_depth, max_nodes, max_work)
}

fn nested_array(depth: usize) -> CfdInputValue {
    (0..depth).fold(CfdInputValue::from(1_i64), |value, _| {
        CfdInputValue::Array(vec![value])
    })
}

fn build_nested_array(
    schema: &CftSchema,
    structural_limits: StructuralLimits,
) -> Result<CfdDataModel, CfdDiagnostics> {
    let mut builder = CfdDataModel::builder(schema).with_structural_limits(structural_limits);
    builder.add_record("item", "Item", [("value", nested_array(3))]);
    builder.build()
}

#[test]
fn nested_value_depth_accepts_boundary_and_rejects_first_deeper_value() {
    let schema = compile_schema("type Item { value: [[[int]]]; }");

    build_nested_array(&schema, limits(5, 100, 100))
        .expect("record root, three arrays, and scalar fit depth five");
    let diagnostics = build_nested_array(&schema, limits(4, 100, 100))
        .expect_err("scalar at depth five must be rejected");
    let diagnostic = diagnostic_with_code(&diagnostics, CfdErrorCode::DataStructureLimitExceeded);
    assert_eq!(
        diagnostic.message,
        "data value exceeds structural depth limit 4 (observed 5)"
    );
    assert_eq!(
        diagnostic.primary.as_ref().map(|label| &label.path),
        Some(&CfdPath::root().field("value").index(0).index(0).index(0))
    );
}

#[test]
fn materialized_node_limit_reports_the_first_rejected_array_item() {
    let schema = compile_schema("type Item { nums: [int]; }");
    let build = |max_nodes| {
        let mut builder =
            CfdDataModel::builder(&schema).with_structural_limits(limits(10, max_nodes, 100));
        builder.add_record(
            "item",
            "Item",
            [(
                "nums",
                CfdInputValue::Array(vec![1_i64.into(), 2_i64.into()]),
            )],
        );
        builder.build()
    };

    build(4).expect("root, array, and two items fit node boundary");
    let diagnostics = build(3).expect_err("fourth materialized node must fail");
    let diagnostic = diagnostic_with_code(&diagnostics, CfdErrorCode::DataStructureLimitExceeded);
    assert_eq!(
        diagnostic.primary.as_ref().map(|label| &label.path),
        Some(&CfdPath::root().field("nums").index(1))
    );
}

#[test]
fn structural_budget_is_independent_for_each_top_level_record() {
    let schema = compile_schema("type Item { value: int; }");
    let mut builder = CfdDataModel::builder(&schema).with_structural_limits(limits(2, 2, 4));
    builder.add_record("first", "Item", [("value", CfdInputValue::from(1_i64))]);
    builder.add_record("second", "Item", [("value", CfdInputValue::from(2_i64))]);

    let model = builder
        .build()
        .expect("one record must not consume another record's budget");
    assert_eq!(model.record_count(), 2);
}

fn spread_chain_model(schema: &CftSchema, max_work: u64) -> Result<CfdDataModel, CfdDiagnostics> {
    let mut builder =
        CfdDataModel::builder(schema).with_structural_limits(limits(100, 100, max_work));
    builder.add_record("base", "Stats", [("hp", CfdInputValue::from(1_i64))]);
    for (key, source) in [("mid1", "base"), ("mid2", "mid1"), ("leaf", "mid2")] {
        builder.add_input_record(CfdInputRecord::with_spreads(
            key,
            "Stats",
            [CfdInputValue::record_ref(source)],
            std::iter::empty::<(&str, CfdInputValue)>(),
        ));
    }
    builder.build()
}

#[test]
fn acyclic_spread_work_is_bounded_without_partial_model_success() {
    let schema = compile_schema("type Stats { hp: int; }");

    spread_chain_model(&schema, 5).expect("five resolution steps fit the boundary");
    let diagnostics = spread_chain_model(&schema, 4)
        .expect_err("the fifth spread resolution step must abort the build");
    let diagnostic = diagnostic_with_code(&diagnostics, CfdErrorCode::DataStructureLimitExceeded);
    assert_eq!(
        diagnostic.message,
        "data value exceeds structural work limit 4 (observed 5)"
    );
    assert_eq!(
        diagnostic.primary.as_ref().map(|label| &label.path),
        Some(&CfdPath::root().field("hp"))
    );
}

#[test]
fn short_spread_and_default_cycles_keep_domain_cycle_diagnostics() {
    let spread_schema = compile_schema("type Stats { hp: int; }");
    let mut spread_builder =
        CfdDataModel::builder(&spread_schema).with_structural_limits(limits(3, 100, 100));
    spread_builder.add_input_record(CfdInputRecord::with_spreads(
        "a",
        "Stats",
        [CfdInputValue::record_ref("b")],
        std::iter::empty::<(&str, CfdInputValue)>(),
    ));
    spread_builder.add_input_record(CfdInputRecord::with_spreads(
        "b",
        "Stats",
        [CfdInputValue::record_ref("a")],
        std::iter::empty::<(&str, CfdInputValue)>(),
    ));
    let spread_diagnostics = spread_builder.build().expect_err("spread cycle must fail");
    assert_has_code(&spread_diagnostics, CfdErrorCode::ValueDependencyCycle);
    assert!(!spread_diagnostics
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == CfdErrorCode::DataStructureLimitExceeded));

    let default_schema = compile_schema("type Node { child: Node = {}; }");
    let mut default_builder =
        CfdDataModel::builder(&default_schema).with_structural_limits(limits(1, 100, 100));
    default_builder.add_record("node", "Node", std::iter::empty::<(&str, CfdInputValue)>());
    let default_diagnostics = default_builder
        .build()
        .expect_err("known default cycle must fail before recursive expansion");
    assert_has_code(&default_diagnostics, CfdErrorCode::ValueDependencyCycle);
    assert!(!default_diagnostics
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == CfdErrorCode::DataStructureLimitExceeded));
}

#[test]
fn cached_default_subtree_is_charged_before_it_is_cloned() {
    let schema = compile_schema(
        "type Leaf { a: int = 1; b: int = 2; } type Root { left: Leaf = {}; right: Leaf = {}; }",
    );
    let build = |max_nodes| {
        let mut builder =
            CfdDataModel::builder(&schema).with_structural_limits(limits(10, max_nodes, 100));
        builder.add_record("root", "Root", std::iter::empty::<(&str, CfdInputValue)>());
        builder.build()
    };

    build(7).expect("both default object copies fit the materialized node boundary");
    let diagnostics = build(6).expect_err("cached subtree copy must consume node budget");
    let diagnostic = diagnostic_with_code(&diagnostics, CfdErrorCode::DataStructureLimitExceeded);
    assert_eq!(
        diagnostic.message,
        "default value exceeds structural nodes limit 6 (observed 7)"
    );
    assert_eq!(
        diagnostic.primary.as_ref().map(|label| &label.path),
        Some(&CfdPath::root().field("right"))
    );
}
