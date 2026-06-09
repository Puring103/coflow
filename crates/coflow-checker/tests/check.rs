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

#[test]
fn check_runner_accepts_core_expressions_refs_and_quantifiers() {
    let schema = compile_schema(
        r#"
            const MIN_LEVEL = 1;
            enum Rarity { Common, Rare, }

            type Item {
                @id
                id: string;
                rarity: Rarity;
            }

            type Drop {
                @ref(Item)
                item_id: string;
                weights: [int];
                resistances: {Rarity: float};

                check {
                    item_id.id != "";
                    item_id.rarity >= Rarity.Common;
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
        "Item",
        [
            ("id", CfdInputValue::from("item_1")),
            ("rarity", CfdInputValue::enum_variant("Rarity", "Rare")),
        ],
    );
    builder.add_record(
        "Drop",
        [
            ("item_id", CfdInputValue::from("item_1")),
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
    let model = builder.build().expect("data model should build");
    model.run_checks(&schema).expect("checks should pass");
}

#[test]
fn check_runner_reports_false_conditions() {
    let schema = compile_schema(
        r#"
            type Item {
                value: int;
                check { value > 0; }
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("Item", [("value", CfdInputValue::from(0_i64))]);
    let model = builder.build().expect("data model should build");
    let err = model.run_checks(&schema).expect_err("check should fail");
    assert_has_code(&err, CfdErrorCode::CheckFailed);
}

#[test]
fn check_runner_short_circuits_nullable_guards_and_reports_null_access() {
    let guarded = compile_schema(
        r#"
            type Child { id: string; }
            type Holder {
                child: Child? = null;
                check { child == null || child.id != ""; }
            }
        "#,
    );
    let mut guarded_builder = CfdDataModel::builder(&guarded);
    guarded_builder.add_input_record(CfdInputRecord::new(
        "Holder",
        std::iter::empty::<(&str, CfdInputValue)>(),
    ));
    let guarded_model = guarded_builder.build().expect("data model should build");
    guarded_model
        .run_checks(&guarded)
        .expect("guarded check should pass");

    let unguarded = compile_schema(
        r#"
            type Child { id: string; }
            type Holder {
                child: Child? = null;
                check { child.id != ""; }
            }
        "#,
    );
    let mut unguarded_builder = CfdDataModel::builder(&unguarded);
    unguarded_builder.add_input_record(CfdInputRecord::new(
        "Holder",
        std::iter::empty::<(&str, CfdInputValue)>(),
    ));
    let unguarded_model = unguarded_builder.build().expect("data model should build");
    let err = unguarded_model
        .run_checks(&unguarded)
        .expect_err("null access");
    assert_has_code(&err, CfdErrorCode::CheckNullAccess);
}

#[test]
fn null_arithmetic_and_ordered_comparison_report_null_access() {
    let arithmetic = compile_schema(
        r#"
            type Item {
                value: int? = null;
                check { value + 1 > 0; }
            }
        "#,
    );
    let mut arithmetic_builder = CfdDataModel::builder(&arithmetic);
    arithmetic_builder.add_input_record(CfdInputRecord::new(
        "Item",
        std::iter::empty::<(&str, CfdInputValue)>(),
    ));
    let arithmetic_model = arithmetic_builder.build().expect("data model should build");
    let arithmetic_err = arithmetic_model
        .run_checks(&arithmetic)
        .expect_err("null arithmetic should fail");
    assert_has_code(&arithmetic_err, CfdErrorCode::CheckNullAccess);

    let comparison = compile_schema(
        r#"
            type Item {
                value: int? = null;
                check { value > 0; }
            }
        "#,
    );
    let mut comparison_builder = CfdDataModel::builder(&comparison);
    comparison_builder.add_input_record(CfdInputRecord::new(
        "Item",
        std::iter::empty::<(&str, CfdInputValue)>(),
    ));
    let comparison_model = comparison_builder.build().expect("data model should build");
    let comparison_err = comparison_model
        .run_checks(&comparison)
        .expect_err("null ordered comparison should fail");
    assert_has_code(&comparison_err, CfdErrorCode::CheckNullAccess);
}

#[test]
fn check_runner_executes_inherited_checks() {
    let schema = compile_schema(
        r#"
            abstract type Reward {
                id: string;
                check { id != ""; }
            }
            type CurrencyReward : Reward {
                amount: int;
                check { amount > 0; }
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "CurrencyReward",
        [
            ("id", CfdInputValue::from("")),
            ("amount", CfdInputValue::from(0_i64)),
        ],
    );
    let model = builder.build().expect("data model should build");
    let err = model
        .run_checks(&schema)
        .expect_err("inherited checks fail");
    let failures = err
        .diagnostics
        .iter()
        .filter(|diag| diag.code == CfdErrorCode::CheckFailed)
        .count();
    assert_eq!(failures, 2);
}

#[test]
fn check_runner_reports_index_and_empty_minmax_eval_errors() {
    let schema = compile_schema(
        r#"
            type Item {
                xs: [int];
                check {
                    xs[1] > 0;
                    min(xs) > 0;
                }
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("Item", [("xs", CfdInputValue::Array(Vec::new()))]);
    let model = builder.build().expect("data model should build");
    let err = model.run_checks(&schema).expect_err("eval errors");
    assert_has_code(&err, CfdErrorCode::CheckIndexOutOfBounds);
    assert!(
        !err.diagnostics
            .iter()
            .any(|diag| diag.code == CfdErrorCode::CheckEmptyMinMax),
        "hard eval errors should stop later statements on the same object"
    );
}

#[test]
fn check_runner_reports_empty_minmax_eval_errors() {
    let schema = compile_schema(
        r#"
            type Item {
                xs: [int];
                check { min(xs) > 0; }
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("Item", [("xs", CfdInputValue::Array(Vec::new()))]);
    let model = builder.build().expect("data model should build");
    let err = model.run_checks(&schema).expect_err("eval errors");
    assert_has_code(&err, CfdErrorCode::CheckEmptyMinMax);
}

#[test]
fn any_quantifier_preserves_hard_eval_errors() {
    let schema = compile_schema(
        r#"
            type Item {
                rows: [[int]];
                check { any row in rows { row[0] > 0; } }
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "Item",
        [(
            "rows",
            CfdInputValue::Array(vec![
                CfdInputValue::Array(Vec::new()),
                CfdInputValue::Array(vec![CfdInputValue::from(1_i64)]),
            ]),
        )],
    );
    let model = builder.build().expect("data model should build");
    let err = model
        .run_checks(&schema)
        .expect_err("hard eval error should not be swallowed");
    assert_has_code(&err, CfdErrorCode::CheckIndexOutOfBounds);
}

#[test]
fn none_quantifier_preserves_hard_eval_errors() {
    let schema = compile_schema(
        r#"
            type Item {
                rows: [[int]];
                check { none row in rows { row[0] > 0; } }
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "Item",
        [(
            "rows",
            CfdInputValue::Array(vec![
                CfdInputValue::Array(Vec::new()),
                CfdInputValue::Array(vec![CfdInputValue::from(-1_i64)]),
            ]),
        )],
    );
    let model = builder.build().expect("data model should build");
    let err = model
        .run_checks(&schema)
        .expect_err("hard eval error should not be swallowed");
    assert_has_code(&err, CfdErrorCode::CheckIndexOutOfBounds);
}

#[test]
fn check_runner_executes_inline_object_checks() {
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
        "Monster",
        [(
            "stats",
            CfdInputValue::object_with_declared_type([("hp", CfdInputValue::from(0_i64))]),
        )],
    );
    let model = builder.build().expect("data model should build");
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
fn any_and_none_quantifiers_do_not_leak_trial_failures() {
    let any_schema = compile_schema(
        r#"
            type Item {
                values: [int];
                check { any value in values { value > 0; } }
            }
        "#,
    );
    let mut any_builder = CfdDataModel::builder(&any_schema);
    any_builder.add_record(
        "Item",
        [(
            "values",
            CfdInputValue::Array(vec![
                CfdInputValue::from(-1_i64),
                CfdInputValue::from(1_i64),
            ]),
        )],
    );
    let any_model = any_builder.build().expect("data model should build");
    any_model
        .run_checks(&any_schema)
        .expect("any should pass without leaking first failed trial");

    let none_schema = compile_schema(
        r#"
            type Item {
                values: [int];
                check { none value in values { value > 0; } }
            }
        "#,
    );
    let mut none_builder = CfdDataModel::builder(&none_schema);
    none_builder.add_record(
        "Item",
        [(
            "values",
            CfdInputValue::Array(vec![
                CfdInputValue::from(-1_i64),
                CfdInputValue::from(-2_i64),
            ]),
        )],
    );
    let none_model = none_builder.build().expect("data model should build");
    none_model
        .run_checks(&none_schema)
        .expect("none should pass without leaking trial failures");
}

#[test]
fn flag_enum_bitwise_composite_has_no_fake_variant_name() {
    // Regression for B3: bitwise OR over @flag enum values may produce a
    // composite (e.g. Read | Write = 3) that has no single declared variant.
    // The runtime used to fabricate `variant = "3"`, breaking downstream
    // codegen / JSON which assume `variant` is an identifier. The composite
    // must round-trip through `is_match`-style checks while keeping
    // `variant: None`.
    let schema = compile_schema(
        r#"
            @flag
            enum Permission { Read = 1, Write = 2, Execute = 4, }
            type Door {
                granted: Permission;
                check {
                    (granted & Permission.Read) != Permission(0);
                    (granted | Permission.Execute) != Permission(0);
                }
            }
        "#,
    );
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "Door",
        [("granted", CfdInputValue::enum_variant("Permission", "Read"))],
    );
    let model = builder.build().expect("data model should build");
    model
        .run_checks(&schema)
        .expect("flag enum bitwise composites should evaluate cleanly");
}

#[test]
fn sum_reports_integer_overflow_consistently_with_other_arithmetic() {
    // Regression for B2: sum used to silently saturate while +/- /etc.
    // raise CheckEvalTypeError. A condition like `sum(weights) == 100`
    // would then pass against a saturating i64::MAX, masking real overflow.
    let schema = compile_schema(
        r#"
            type Item {
                xs: [int];
                check { sum(xs) > 0; }
            }
        "#,
    );
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "Item",
        [(
            "xs",
            CfdInputValue::Array(vec![
                CfdInputValue::from(i64::MAX),
                CfdInputValue::from(1_i64),
            ]),
        )],
    );
    let model = builder.build().expect("data model should build");
    let err = model.run_checks(&schema).expect_err("overflow eval error");
    assert_has_code(&err, CfdErrorCode::CheckEvalTypeError);
}

#[test]
fn sum_of_empty_float_array_uses_declared_element_type() {
    let schema = compile_schema(
        r#"
            type Item {
                xs: [float] = [];
                check { sum(xs) == 0.0; }
            }
        "#,
    );
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_input_record(CfdInputRecord::new(
        "Item",
        std::iter::empty::<(&str, CfdInputValue)>(),
    ));
    let model = builder.build().expect("data model should build");
    model
        .run_checks(&schema)
        .expect("empty float sum should evaluate as 0.0");
}

#[test]
fn arithmetic_eval_errors_are_reported_without_panicking() {
    let schema = compile_schema(
        r#"
            type Item {
                value: int;
                check {
                    value / 0 > 0;
                    value % 0 == 0;
                    value ** -1 > 0;
                }
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("Item", [("value", CfdInputValue::from(1_i64))]);
    let model = builder.build().expect("data model should build");
    let result = std::panic::catch_unwind(|| model.run_checks(&schema));
    assert!(result.is_ok(), "check runner should not panic");
    let err = result.unwrap().expect_err("arithmetic eval errors");
    assert_has_code(&err, CfdErrorCode::CheckEvalTypeError);
}

#[test]
fn check_runner_executes_inline_object_checks_inside_collections() {
    let schema = compile_schema(
        r#"
            type Stat {
                hp: int;
                check { hp > 0; }
            }
            type Monster {
                stats: [Stat];
                named: {string: Stat};
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "Monster",
        [
            (
                "stats",
                CfdInputValue::Array(vec![CfdInputValue::object_with_declared_type([(
                    "hp",
                    CfdInputValue::from(0_i64),
                )])]),
            ),
            (
                "named",
                CfdInputValue::dict([(
                    CfdInputDictKey::from("bad"),
                    CfdInputValue::object_with_declared_type([("hp", CfdInputValue::from(-1_i64))]),
                )]),
            ),
        ],
    );
    let model = builder.build().expect("data model should build");
    let err = model.run_checks(&schema).expect_err("nested checks fail");
    let failed_paths = err
        .diagnostics
        .iter()
        .filter(|diag| diag.code == CfdErrorCode::CheckFailed)
        .filter_map(|diag| diag.primary.as_ref().map(|label| label.path.clone()))
        .collect::<Vec<_>>();
    assert!(failed_paths.contains(&CfdPath::root().field("stats").index(0).field("hp")));
    // Regression for B1: dict-key path used to be the entry index ("0"),
    // hiding the actual key. Now it must be the formatted key form
    // (`"bad"` for a string key) so users can locate the failing entry.
    assert!(failed_paths.contains(
        &CfdPath::root()
            .field("named")
            .dict_key("\"bad\"".to_string())
            .field("hp"),
    ));
}

#[test]
fn any_and_none_quantifier_failures_report_element_failures() {
    let any_schema = compile_schema(
        r#"
            type Item {
                values: [int];
                check { any value in values { value > 0; } }
            }
        "#,
    );
    let mut any_builder = CfdDataModel::builder(&any_schema);
    any_builder.add_record(
        "Item",
        [(
            "values",
            CfdInputValue::Array(vec![
                CfdInputValue::from(-1_i64),
                CfdInputValue::from(-2_i64),
            ]),
        )],
    );
    let any_model = any_builder.build().expect("data model should build");
    let any_err = any_model.run_checks(&any_schema).expect_err("any fails");
    assert_eq!(any_err.diagnostics.len(), 2);
    assert!(any_err
        .diagnostics
        .iter()
        .all(|diag| diag.code == CfdErrorCode::CheckFailed));
    let any_paths = any_err
        .diagnostics
        .iter()
        .filter_map(|diag| diag.primary.as_ref().map(|label| label.path.clone()))
        .collect::<Vec<_>>();
    assert!(any_paths.contains(&CfdPath::root().field("values").index(0)));
    assert!(any_paths.contains(&CfdPath::root().field("values").index(1)));

    let none_schema = compile_schema(
        r#"
            type Item {
                values: [int];
                check { none value in values { value > 0; } }
            }
        "#,
    );
    let mut none_builder = CfdDataModel::builder(&none_schema);
    none_builder.add_record(
        "Item",
        [(
            "values",
            CfdInputValue::Array(vec![CfdInputValue::from(1_i64), CfdInputValue::from(2_i64)]),
        )],
    );
    let none_model = none_builder.build().expect("data model should build");
    let none_err = none_model.run_checks(&none_schema).expect_err("none fails");
    assert_eq!(none_err.diagnostics.len(), 2);
    assert!(none_err
        .diagnostics
        .iter()
        .all(|diag| diag.code == CfdErrorCode::CheckFailed));
    let none_paths = none_err
        .diagnostics
        .iter()
        .filter_map(|diag| diag.primary.as_ref().map(|label| label.path.clone()))
        .collect::<Vec<_>>();
    assert!(none_paths.contains(&CfdPath::root().field("values").index(0)));
    assert!(none_paths.contains(&CfdPath::root().field("values").index(1)));
}
