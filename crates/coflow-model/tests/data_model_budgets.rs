#![allow(clippy::expect_used, clippy::panic_in_result_fn)]

#[path = "data_model_common/mod.rs"]
mod common;
use common::*;

const fn limits(max_depth: u64, max_nodes: u64, max_work: u64) -> StructuralLimits {
    StructuralLimits::new(max_depth, max_nodes, max_work)
}

fn nested_array(depth: usize) -> LoadedValueDraft {
    (0..depth).fold(LoadedValueDraft::from(1_i64), |value, _| {
        LoadedValueDraft::Array(vec![value])
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
    let schema = compile_schema("table Item { value: [[[int]]]; }");

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
    let schema = compile_schema("table Item { nums: [int]; }");
    let build = |max_nodes| {
        let mut builder =
            CfdDataModel::builder(&schema).with_structural_limits(limits(10, max_nodes, 100));
        builder.add_record(
            "item",
            "Item",
            [(
                "nums",
                LoadedValueDraft::Array(vec![1_i64.into(), 2_i64.into()]),
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
fn explicit_schema_default_values_share_the_input_value_budget() {
    let schema = compile_schema("table Item { nums: [int] = [1, 2]; }");
    let build = |max_nodes, supply_value: bool| {
        let mut builder =
            CfdDataModel::builder(&schema).with_structural_limits(limits(10, max_nodes, 100));
        let fields = supply_value.then(|| {
            (
                "nums",
                LoadedValueDraft::Array(vec![1_i64.into(), 2_i64.into()]),
            )
        });
        builder.add_record("item", "Item", fields);
        builder.build()
    };
    build(4, false).expect("declared array default fits four nodes");
    let default_error = build(3, false).expect_err("default elements consume the budget");
    let input_error = build(3, true).expect_err("explicit elements consume the same budget");
    let default = diagnostic_with_code(&default_error, CfdErrorCode::DataStructureLimitExceeded);
    let input = diagnostic_with_code(&input_error, CfdErrorCode::DataStructureLimitExceeded);
    assert_eq!(default.message, input.message);
    assert_eq!(
        default.primary.as_ref().map(|label| &label.path),
        input.primary.as_ref().map(|label| &label.path)
    );
    assert_eq!(
        default.primary.as_ref().map(|label| &label.path),
        Some(&CfdPath::root().field("nums").index(1))
    );
}

#[test]
fn reused_default_inputs_charge_every_record_and_preserve_the_first_rejected_item() {
    let schema = compile_schema("table Item { prefix: [int]; nums: [int] = [1, 2]; }");
    let build = |max_nodes, explicit_second: bool| {
        let mut builder =
            CfdDataModel::builder(&schema).with_structural_limits(limits(10, max_nodes, 100));
        builder.add_record(
            "first",
            "Item",
            [("prefix", LoadedValueDraft::Array(Vec::new()))],
        );
        let mut fields = vec![("prefix", LoadedValueDraft::Array(vec![0_i64.into()]))];
        if explicit_second {
            fields.push((
                "nums",
                LoadedValueDraft::Array(vec![1_i64.into(), 2_i64.into()]),
            ));
        }
        builder.add_record("second", "Item", fields);
        builder.build()
    };
    build(6, false).expect("each record has an independent budget");
    let cached = build(5, false).expect_err("reused default input still consumes every node");
    let explicit = build(5, true).expect_err("explicit input has the same budget");
    let cached = diagnostic_with_code(&cached, CfdErrorCode::DataStructureLimitExceeded);
    let explicit = diagnostic_with_code(&explicit, CfdErrorCode::DataStructureLimitExceeded);
    assert_eq!(cached.message, explicit.message);
    let cached = cached.primary.as_ref().expect("cached default location");
    let explicit = explicit.primary.as_ref().expect("explicit input location");
    assert_eq!(cached.record, explicit.record);
    assert_eq!(cached.record.map(CfdRecordHandle::index), Some(1));
    assert_eq!(cached.path, explicit.path);
    assert_eq!(cached.path, CfdPath::root().field("nums").index(1));
}

#[test]
fn explicit_object_defaults_report_the_first_field_beyond_the_budget() {
    let schema = compile_schema(
        "data Child { value: int; } table Item { child: Child = Child { value: 1 }; }",
    );
    let build = |max_nodes| {
        let mut builder =
            CfdDataModel::builder(&schema).with_structural_limits(limits(10, max_nodes, 100));
        builder.add_record(
            "item",
            "Item",
            std::iter::empty::<(&str, LoadedValueDraft)>(),
        );
        builder.build()
    };
    build(3).expect("root, object and explicit field fit three nodes");
    let diagnostics = build(2).expect_err("explicit default field consumes the third node");
    let diagnostic = diagnostic_with_code(&diagnostics, CfdErrorCode::DataStructureLimitExceeded);
    assert_eq!(
        diagnostic.primary.as_ref().map(|label| &label.path),
        Some(&CfdPath::root().field("child").field("value"))
    );
}

#[test]
fn structural_budget_is_independent_for_each_top_level_record() {
    let schema = compile_schema("table Item { value: int; }");
    let mut builder = CfdDataModel::builder(&schema).with_structural_limits(limits(2, 2, 4));
    builder.add_record("first", "Item", [("value", LoadedValueDraft::from(1_i64))]);
    builder.add_record("second", "Item", [("value", LoadedValueDraft::from(2_i64))]);

    let model = builder
        .build()
        .expect("one record must not consume another record's budget");
    assert_eq!(model.record_count(), 2);
}

#[test]
fn cached_default_subtree_is_charged_before_it_is_cloned() {
    let schema = compile_schema(
        "data Leaf { a: int = 1; b: int = 2; } table Root { left: Leaf = {}; right: Leaf = {}; }",
    );
    let build = |max_nodes| {
        let mut builder =
            CfdDataModel::builder(&schema).with_structural_limits(limits(10, max_nodes, 100));
        builder.add_record(
            "root",
            "Root",
            std::iter::empty::<(&str, LoadedValueDraft)>(),
        );
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
