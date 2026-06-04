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
    assert_has_code(&err, CfdErrorCode::CheckEmptyMinMax);
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
        Some(CfdPath::root().field("stats"))
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
    assert!(failed_paths.contains(&CfdPath::root().field("stats").index(0)));
    assert!(failed_paths.contains(&CfdPath::root().field("named").dict_key("0")));
}

#[test]
fn any_and_none_quantifier_failures_report_quantifier_only() {
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
    assert_eq!(any_err.diagnostics.len(), 1);
    assert_eq!(any_err.diagnostics[0].code, CfdErrorCode::CheckFailed);
    assert_eq!(
        any_err.diagnostics[0].message,
        "any quantifier did not match any element"
    );

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
            CfdInputValue::Array(vec![CfdInputValue::from(1_i64)]),
        )],
    );
    let none_model = none_builder.build().expect("data model should build");
    let none_err = none_model.run_checks(&none_schema).expect_err("none fails");
    assert_eq!(none_err.diagnostics.len(), 1);
    assert_eq!(none_err.diagnostics[0].code, CfdErrorCode::CheckFailed);
    assert_eq!(
        none_err.diagnostics[0].message,
        "none quantifier matched at least one element"
    );
}
