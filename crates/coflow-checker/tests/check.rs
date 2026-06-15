#![allow(
    clippy::expect_used,
    clippy::needless_raw_string_hashes,
    clippy::panic,
    clippy::panic_in_result_fn,
    clippy::too_many_lines,
    clippy::unwrap_used
)]

mod common;
use common::*;

fn build_model(_schema: &CftContainer, builder: CfdModelBuilder<'_>) -> CfdDataModel {
    builder.build().expect("data model should build")
}

#[test]
fn check_runner_accepts_virtual_ids_record_refs_and_quantifiers() {
    let schema = compile_schema(
        r#"
            const MIN_LEVEL = 1;
            enum Rarity { Common, Rare, }

            type Item {
                rarity: Rarity;
                check {
                    id != "";
                    rarity >= Rarity.Common;
                }
            }

            type Drop {
                item: Item;
                weights: [int];
                resistances: {Rarity: float};

                check {
                    id == "drop_1";
                    item.id == "item_1";
                    item.rarity >= Rarity.Common;
                    len(weights) >= MIN_LEVEL;
                    sum(weights) == 100;
                    all entry in resistances {
                        entry.key >= Rarity.Common;
                        entry.value >= 0.0;
                    }
                    contains(keys(resistances), Rarity.Rare);
                }
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "item_1",
        "Item",
        [("rarity", CfdInputValue::enum_variant("Rarity", "Rare"))],
    );
    builder.add_record(
        "drop_1",
        "Drop",
        [
            ("item", CfdInputValue::record_ref("item_1")),
            (
                "weights",
                CfdInputValue::Array(vec![
                    CfdInputValue::from(40_i64),
                    CfdInputValue::from(60_i64),
                ]),
            ),
            (
                "resistances",
                CfdInputValue::dict([
                    (
                        CfdInputDictKey::enum_variant("Rarity", "Common"),
                        CfdInputValue::from(0.5_f64),
                    ),
                    (
                        CfdInputDictKey::enum_variant("Rarity", "Rare"),
                        CfdInputValue::from(1.0_f64),
                    ),
                ]),
            ),
        ],
    );
    let model = build_model(&schema, builder);
    model.run_checks(&schema).expect("checks should pass");
}

#[test]
fn check_runner_reports_false_conditions_with_paths() {
    let schema = compile_schema(
        r#"
            type Item {
                value: int;
                check { value > 0; }
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("item_1", "Item", [("value", CfdInputValue::from(0_i64))]);
    let model = build_model(&schema, builder);
    let err = model.run_checks(&schema).expect_err("check should fail");
    assert_has_code(&err, CfdErrorCode::CheckFailed);
    assert_eq!(
        err.diagnostics[0]
            .primary
            .as_ref()
            .map(|label| label.path.clone()),
        Some(CfdPath::root().field("value"))
    );
}

#[test]
fn logical_and_bitwise_precedence_remains_left_associative() {
    let logical = compile_schema(
        r#"
            type Item { check { true || false && false; } }
        "#,
    );
    let mut builder = CfdDataModel::builder(&logical);
    builder.add_record(
        "item_1",
        "Item",
        std::iter::empty::<(&str, CfdInputValue)>(),
    );
    let model = build_model(&logical, builder);
    let err = model
        .run_checks(&logical)
        .expect_err("same-precedence logical operators evaluate left-to-right");
    assert_has_code(&err, CfdErrorCode::CheckFailed);

    let bitwise = compile_schema(
        r#"
            type Item { check { 1 | 2 & 0 == 0; } }
        "#,
    );
    let mut builder = CfdDataModel::builder(&bitwise);
    builder.add_record(
        "item_1",
        "Item",
        std::iter::empty::<(&str, CfdInputValue)>(),
    );
    let model = build_model(&bitwise, builder);
    model
        .run_checks(&bitwise)
        .expect("same-precedence bitwise operators evaluate left-to-right");
}

#[test]
fn short_circuit_nullable_guards_and_null_access_are_reported() {
    let guarded = compile_schema(
        r#"
            type Child { name: string; }
            type Holder {
                child: Child? = null;
                check { child == null || child.name != ""; }
            }
        "#,
    );
    let mut builder = CfdDataModel::builder(&guarded);
    builder.add_record(
        "holder_1",
        "Holder",
        std::iter::empty::<(&str, CfdInputValue)>(),
    );
    let model = build_model(&guarded, builder);
    model
        .run_checks(&guarded)
        .expect("guarded check should pass");

    let unguarded = compile_schema(
        r#"
            type Child { name: string; }
            type Holder {
                child: Child? = null;
                check { child.name != ""; }
            }
        "#,
    );
    let mut builder = CfdDataModel::builder(&unguarded);
    builder.add_record(
        "holder_1",
        "Holder",
        std::iter::empty::<(&str, CfdInputValue)>(),
    );
    let model = build_model(&unguarded, builder);
    let err = model.run_checks(&unguarded).expect_err("null access");
    assert_has_code(&err, CfdErrorCode::CheckNullAccess);
}

#[test]
fn nullable_element_builtins_handle_nulls_and_empty_values() {
    let pass = compile_schema(
        r#"
            type Holder {
                nums: [int?] = [];
                check {
                    unique(nums);
                    min(nums) == 1;
                    max(nums) == 3;
                    sum(nums) == 4;
                    contains(nums, null);
                    len(nums) == 3;
                }
            }
        "#,
    );
    let mut builder = CfdDataModel::builder(&pass);
    builder.add_record(
        "holder_1",
        "Holder",
        [(
            "nums",
            CfdInputValue::Array(vec![
                CfdInputValue::from(1_i64),
                CfdInputValue::Null,
                CfdInputValue::from(3_i64),
            ]),
        )],
    );
    let model = build_model(&pass, builder);
    model.run_checks(&pass).expect("checks should pass");

    let empty = compile_schema(
        r#"
            type Holder {
                nums: [int?] = [];
                check { min(nums) >= 0; }
            }
        "#,
    );
    let mut builder = CfdDataModel::builder(&empty);
    builder.add_record(
        "holder_1",
        "Holder",
        [("nums", CfdInputValue::Array(vec![CfdInputValue::Null]))],
    );
    let model = build_model(&empty, builder);
    let err = model
        .run_checks(&empty)
        .expect_err("min over all-null values");
    assert_has_code(&err, CfdErrorCode::CheckEmptyMinMax);
}

#[test]
fn inherited_checks_and_statement_order_are_stable() {
    let schema = compile_schema(
        r#"
            abstract type Base {
                check { id != ""; }
            }
            type Child : Base {
                first: int;
                second: int;
                check {
                    first > 0;
                    second > 0;
                }
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "",
        "Child",
        [
            ("first", CfdInputValue::from(0_i64)),
            ("second", CfdInputValue::from(0_i64)),
        ],
    );
    let err = builder
        .build()
        .expect_err("empty record key should fail before checks");
    assert_has_code(&err, CfdErrorCode::MissingIdField);

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "child_1",
        "Child",
        [
            ("first", CfdInputValue::from(0_i64)),
            ("second", CfdInputValue::from(0_i64)),
        ],
    );
    let model = build_model(&schema, builder);
    let err = model.run_checks(&schema).expect_err("child checks fail");
    let paths = err
        .diagnostics
        .iter()
        .filter(|diag| diag.code == CfdErrorCode::CheckFailed)
        .filter_map(|diag| diag.primary.as_ref().map(|label| label.path.clone()))
        .collect::<Vec<_>>();
    assert_eq!(
        paths,
        vec![
            CfdPath::root().field("first"),
            CfdPath::root().field("second"),
        ]
    );
}

#[test]
fn hard_stop_in_one_check_block_does_not_skip_later_blocks() {
    let schema = compile_schema(
        r#"
            abstract type Base {
                xs: [int];
                check { xs[0] > 0; }
            }

            type Item : Base {
                value: int;
                check { value > 0; }
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "item_1",
        "Item",
        [
            ("xs", CfdInputValue::Array(Vec::new())),
            ("value", CfdInputValue::from(0_i64)),
        ],
    );
    let model = build_model(&schema, builder);
    let err = model.run_checks(&schema).expect_err("checks should fail");

    assert_has_code(&err, CfdErrorCode::CheckIndexOutOfBounds);
    assert_has_code(&err, CfdErrorCode::CheckFailed);
}

#[test]
fn quantifiers_report_soft_failures_and_preserve_hard_errors() {
    let soft_fail = compile_schema(
        r#"
            type Item {
                nums: [int];
                check { all value in nums { value > 0; } }
            }
        "#,
    );
    let mut builder = CfdDataModel::builder(&soft_fail);
    builder.add_record(
        "item_1",
        "Item",
        [(
            "nums",
            CfdInputValue::Array(vec![
                CfdInputValue::from(-1_i64),
                CfdInputValue::from(-2_i64),
            ]),
        )],
    );
    let model = build_model(&soft_fail, builder);
    let err = model
        .run_checks(&soft_fail)
        .expect_err("all reports each failing element");
    let soft_fail_paths = err
        .diagnostics
        .iter()
        .filter(|diag| diag.code == CfdErrorCode::CheckFailed)
        .filter_map(|diag| diag.primary.as_ref().map(|label| label.path.clone()))
        .collect::<Vec<_>>();
    assert_eq!(
        soft_fail_paths,
        vec![
            CfdPath::root().field("nums").index(0),
            CfdPath::root().field("nums").index(1),
        ]
    );

    let hard_stop = compile_schema(
        r#"
            type Item {
                rows: [[int]];
                check { any row in rows { row[0] > 0; } }
            }
        "#,
    );
    let mut builder = CfdDataModel::builder(&hard_stop);
    builder.add_record(
        "item_1",
        "Item",
        [(
            "rows",
            CfdInputValue::Array(vec![
                CfdInputValue::Array(Vec::new()),
                CfdInputValue::Array(vec![CfdInputValue::from(1_i64)]),
            ]),
        )],
    );
    let model = build_model(&hard_stop, builder);
    let err = model
        .run_checks(&hard_stop)
        .expect_err("hard eval error should not be swallowed");
    assert_has_code(&err, CfdErrorCode::CheckIndexOutOfBounds);
}

#[test]
fn inline_object_checks_use_nested_paths() {
    let schema = compile_schema(
        r#"
            type Stats {
                hp: int;
                check { hp > 0; }
            }
            type Monster {
                stats: Stats;
                check { true; }
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "monster_1",
        "Monster",
        [(
            "stats",
            CfdInputValue::object_with_declared_type([("hp", CfdInputValue::from(0_i64))]),
        )],
    );
    let model = build_model(&schema, builder);
    let err = model.run_checks(&schema).expect_err("nested check fails");
    assert_has_code(&err, CfdErrorCode::CheckFailed);
    let diag = err
        .diagnostics
        .iter()
        .find(|diag| diag.code == CfdErrorCode::CheckFailed)
        .expect("check failed diagnostic");
    assert_eq!(
        diag.primary.as_ref().map(|label| label.path.clone()),
        Some(CfdPath::root().field("stats").field("hp"))
    );
}

#[test]
fn flag_enum_bitwise_composites_and_int_ops_work() {
    let schema = compile_schema(
        r#"
            @flag
            enum Permission { Read = 1, Write = 2, Execute = 4, }
            type Door {
                granted: Permission;
                value: int;
                check {
                    (granted & Permission.Read) != Permission(0);
                    (granted | Permission.Execute) != Permission(0);
                    (granted ^ Permission.Write) != Permission(0);
                    (~granted & Permission.Execute) != Permission(0);
                    value // 2 == 3;
                    value % 2 == 1;
                    2 ** 3 == 8;
                }
            }
        "#,
    );
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "door_1",
        "Door",
        [
            ("granted", CfdInputValue::enum_variant("Permission", "Read")),
            ("value", CfdInputValue::from(7_i64)),
        ],
    );
    let model = build_model(&schema, builder);
    model.run_checks(&schema).expect("operators should pass");
}

#[test]
fn runtime_reports_index_dict_and_regex_edges() {
    let negative_index = compile_schema(
        r#"
            type Item {
                nums: [int];
                check { nums[-1] > 0; }
            }
        "#,
    );
    let mut builder = CfdDataModel::builder(&negative_index);
    builder.add_record(
        "item_1",
        "Item",
        [(
            "nums",
            CfdInputValue::Array(vec![CfdInputValue::from(1_i64)]),
        )],
    );
    let model = build_model(&negative_index, builder);
    let err = model
        .run_checks(&negative_index)
        .expect_err("negative index should fail");
    assert_has_code(&err, CfdErrorCode::CheckIndexOutOfBounds);

    let missing_key = compile_schema(
        r#"
            type Item {
                attrs: {string: int};
                check { attrs["missing"] > 0; }
            }
        "#,
    );
    let mut builder = CfdDataModel::builder(&missing_key);
    builder.add_record(
        "item_1",
        "Item",
        [(
            "attrs",
            CfdInputValue::dict([(CfdInputDictKey::from("present"), CfdInputValue::from(1_i64))]),
        )],
    );
    let model = build_model(&missing_key, builder);
    let err = model
        .run_checks(&missing_key)
        .expect_err("missing dict key should fail");
    assert_has_code(&err, CfdErrorCode::CheckMissingDictKey);

    let regex = compile_schema(
        r#"
            type Item {
                label: string;
                check { matches(label, "配置"); }
            }
        "#,
    );
    let mut builder = CfdDataModel::builder(&regex);
    builder.add_record(
        "item_1",
        "Item",
        [("label", CfdInputValue::from("怪物配置"))],
    );
    let model = build_model(&regex, builder);
    model
        .run_checks(&regex)
        .expect("matches should use Unicode regex semantics");
}

#[test]
fn top_level_ref_targets_run_checks_once_by_identity() {
    let schema = compile_schema(
        r#"
            type Target {
                value: int;
                check { value > 0; }
            }

            type Holder {
                first: Target;
                second: Target;
                check { first.id == second.id; }
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "target_1",
        "Target",
        [("value", CfdInputValue::from(0_i64))],
    );
    builder.add_record(
        "holder_1",
        "Holder",
        [
            ("first", CfdInputValue::record_ref("target_1")),
            ("second", CfdInputValue::record_ref("target_1")),
        ],
    );
    let model = build_model(&schema, builder);
    let err = model
        .run_checks(&schema)
        .expect_err("invalid target should fail once");

    let failures = err
        .diagnostics
        .iter()
        .filter(|diag| diag.code == CfdErrorCode::CheckFailed)
        .collect::<Vec<_>>();
    assert_eq!(failures.len(), 1);
    assert_eq!(
        failures[0].primary.as_ref().and_then(|label| label.record),
        Some(record_id_at(&model, 0))
    );
}

#[test]
fn empty_sum_and_float_edge_semantics_are_preserved() {
    let empty_sum = compile_schema(
        r#"
            type Item {
                nums: [int] = [];
                check { sum(nums) == 0; }
            }
        "#,
    );
    let mut builder = CfdDataModel::builder(&empty_sum);
    builder.add_record(
        "item_1",
        "Item",
        std::iter::empty::<(&str, CfdInputValue)>(),
    );
    let model = build_model(&empty_sum, builder);
    model
        .run_checks(&empty_sum)
        .expect("empty int sum should evaluate as 0");

    let float_div_zero = compile_schema(
        r#"
            type Item {
                value: float;
                check { value / 0.0 > 0.0; }
            }
        "#,
    );
    let mut builder = CfdDataModel::builder(&float_div_zero);
    builder.add_record("item_1", "Item", [("value", CfdInputValue::from(1.0_f64))]);
    let model = build_model(&float_div_zero, builder);
    model
        .run_checks(&float_div_zero)
        .expect("float division by zero follows f64 infinity semantics");
}
