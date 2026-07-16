#![allow(
    clippy::expect_used,
    clippy::needless_raw_string_hashes,
    clippy::panic,
    clippy::panic_in_result_fn,
    clippy::too_many_lines,
    clippy::unwrap_used
)]

mod common;
use coflow_checker::{CheckRequest, DependencyCollection};
use common::*;

fn build_model(_schema: &CftSchema, builder: CfdModelBuilder<'_>) -> CfdDataModel {
    builder.build().expect("data model should build")
}

fn assert_first_code(diags: &CfdDiagnostics, code: CfdErrorCode) {
    assert_eq!(diags.diagnostics[0].code, code, "{diags:#?}");
}

fn assert_message_contains(diags: &CfdDiagnostics, text: &str) {
    assert!(
        diags
            .diagnostics
            .iter()
            .any(|diag| diag.message.contains(text)),
        "missing `{text}` in {diags:#?}"
    );
}

#[test]
fn subset_checks_return_only_selected_diagnostics_and_dependencies() {
    let schema = compile_schema(
        r#"
            type Item {
                value: int;
                target: &Item? = null;
                check {
                    value > 0;
                    target == null || target.value > 0;
                }
            }
        "#,
    );
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "target",
        "Item",
        [
            ("value", LoadedValueDraft::from(-1_i64)),
            ("target", LoadedValueDraft::Null),
        ],
    );
    builder.add_record(
        "reader",
        "Item",
        [
            ("value", LoadedValueDraft::from(1_i64)),
            ("target", LoadedValueDraft::record_ref("target")),
        ],
    );
    let model = builder.build().expect("model builds");
    let target = model
        .lookup_assignable(&schema, "Item", "target")
        .expect("target");
    let reader = model
        .lookup_assignable(&schema, "Item", "reader")
        .expect("reader");

    let output = coflow_checker::run_checks(
        &schema,
        &model,
        CheckRequest::records(&[reader]).with_dependency_collection(DependencyCollection::Reads),
    );
    let diagnostics = output.diagnostics;
    let graph = output.dependencies;

    assert!(
        diagnostics.iter().all(|rooted| {
            rooted.root == reader
                && rooted
                    .diagnostic
                    .primary
                    .as_ref()
                    .is_some_and(|primary| primary.record == Some(target))
        }),
        "subset diagnostics: {diagnostics:#?}"
    );
    assert!(graph
        .reads_from
        .get(&reader)
        .is_some_and(|reads| reads.contains(&target)));
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
                item: &Item;
                weights: [int];
                resistances: {Rarity: float};

                check {
                    id == "drop_1";
                    item.id == "item_1";
                    item.rarity >= Rarity.Common;
                    weights.len() >= MIN_LEVEL;
                    weights.sum() == 100;
                    all entry in resistances {
                        entry.key >= Rarity.Common;
                        entry.value >= 0.0;
                    }
                    resistances.keys().contains(Rarity.Rare);
                }
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "item_1",
        "Item",
        [("rarity", LoadedValueDraft::enum_variant("Rarity", "Rare"))],
    );
    builder.add_record(
        "drop_1",
        "Drop",
        [
            ("item", LoadedValueDraft::record_ref("item_1")),
            (
                "weights",
                LoadedValueDraft::Array(vec![
                    LoadedValueDraft::from(40_i64),
                    LoadedValueDraft::from(60_i64),
                ]),
            ),
            (
                "resistances",
                LoadedValueDraft::dict([
                    (
                        LoadedDictKeyDraft::enum_variant("Rarity", "Common"),
                        LoadedValueDraft::from(0.5_f64),
                    ),
                    (
                        LoadedDictKeyDraft::enum_variant("Rarity", "Rare"),
                        LoadedValueDraft::from(1.0_f64),
                    ),
                ]),
            ),
        ],
    );
    let model = build_model(&schema, builder);
    run_model_checks(&model, &schema).expect("checks should pass");
}

#[test]
fn check_diagnostics_use_specific_codes_for_scalar_false_conditions() {
    let schema = compile_schema(
        r#"
            abstract type Reward {}
            type CurrencyReward : Reward {}
            type ItemReward : Reward {}

            type Item {
                level: int;
                enabled: bool;
                negated: bool;
                left: bool;
                right: bool;
                reward: &Reward;
                optional: int? = null;
                tags: [string];
                name: string;
                check {
                    level > 0;
                    enabled;
                    !negated;
                    left && right;
                    left || right;
                    reward is CurrencyReward;
                    optional != null;
                    tags.contains("boss");
                    tags.isUnique();
                    name.matches("^npc_");
                }
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "reward_1",
        "ItemReward",
        std::iter::empty::<(&str, LoadedValueDraft)>(),
    );
    builder.add_record(
        "item_1",
        "Item",
        [
            ("level", LoadedValueDraft::from(0_i64)),
            ("enabled", LoadedValueDraft::from(false)),
            ("negated", LoadedValueDraft::from(true)),
            ("left", LoadedValueDraft::from(false)),
            ("right", LoadedValueDraft::from(false)),
            ("reward", LoadedValueDraft::record_ref("reward_1")),
            (
                "tags",
                LoadedValueDraft::Array(vec![
                    LoadedValueDraft::from("mob"),
                    LoadedValueDraft::from("mob"),
                ]),
            ),
            ("name", LoadedValueDraft::from("mob_1")),
        ],
    );
    let model = build_model(&schema, builder);
    let err = run_model_checks(&model, &schema).expect_err("scalar check diagnostics should fail");

    assert_has_code(&err, CfdErrorCode::CheckComparisonFailed);
    assert_has_code(&err, CfdErrorCode::CheckBoolExpectedTrue);
    assert_has_code(&err, CfdErrorCode::CheckNegationFailed);
    assert_has_code(&err, CfdErrorCode::CheckAndFailed);
    assert_has_code(&err, CfdErrorCode::CheckOrFailed);
    assert_has_code(&err, CfdErrorCode::CheckTypePredicateFailed);
    assert_has_code(&err, CfdErrorCode::CheckNullPredicateFailed);
    assert_has_code(&err, CfdErrorCode::CheckContainsFailed);
    assert_has_code(&err, CfdErrorCode::CheckUniqueFailed);
    assert_has_code(&err, CfdErrorCode::CheckMatchesFailed);

    assert_message_contains(&err, "校验失败: level > 0");
    assert_message_contains(&err, "实际值: level = 0");
    assert_message_contains(&err, "期望: > 0");
    assert_message_contains(&err, "校验失败: tags.contains(\"boss\")");
    assert_message_contains(&err, "期望: 包含 \"boss\"");
    assert_message_contains(&err, "校验失败: name.matches(\"^npc_\")");
}

#[test]
fn check_diagnostics_use_specific_codes_for_quantifiers_and_when_context() {
    let schema = compile_schema(
        r#"
            type Item {
                any_flags: [bool];
                none_flags: [bool];
                all_flags: [bool];
                gated: bool;
                optional: int? = null;
                check {
                    any flag in any_flags { flag; }
                    none flag in none_flags { flag; }
                    all flag in all_flags { flag; }
                    when gated {
                        optional != null;
                    }
                }
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "item_1",
        "Item",
        [
            (
                "any_flags",
                LoadedValueDraft::Array(vec![
                    LoadedValueDraft::from(false),
                    LoadedValueDraft::from(false),
                ]),
            ),
            (
                "none_flags",
                LoadedValueDraft::Array(vec![
                    LoadedValueDraft::from(false),
                    LoadedValueDraft::from(true),
                ]),
            ),
            (
                "all_flags",
                LoadedValueDraft::Array(vec![
                    LoadedValueDraft::from(true),
                    LoadedValueDraft::from(false),
                ]),
            ),
            ("gated", LoadedValueDraft::from(true)),
        ],
    );
    let model = build_model(&schema, builder);
    let err =
        run_model_checks(&model, &schema).expect_err("quantifier and when diagnostics should fail");

    assert_first_code(&err, CfdErrorCode::CheckAnyQuantifierFailed);
    assert_has_code(&err, CfdErrorCode::CheckNoneQuantifierFailed);
    assert_has_code(&err, CfdErrorCode::CheckAllQuantifierFailed);
    assert_has_code(&err, CfdErrorCode::CheckNullPredicateFailed);

    assert_message_contains(&err, "校验失败: any flag in any_flags");
    assert_message_contains(&err, "实际值: 0 / 2 个元素匹配");
    assert_message_contains(&err, "校验失败: none flag in none_flags");
    assert_message_contains(&err, "校验失败: all flag in all_flags");
    assert_message_contains(&err, "上下文: 在 when gated 内");
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
    builder.add_record("item_1", "Item", [("value", LoadedValueDraft::from(0_i64))]);
    let model = build_model(&schema, builder);
    let err = run_model_checks(&model, &schema).expect_err("check should fail");
    assert_has_code(&err, CfdErrorCode::CheckComparisonFailed);
    assert_eq!(
        err.diagnostics[0]
            .primary
            .as_ref()
            .map(|label| label.path.clone()),
        Some(CfdPath::root().field("value"))
    );
}

#[test]
fn logical_and_binds_tighter_than_or_and_bitwise_precedence_remains_left_associative() {
    let logical = compile_schema(
        r#"
            type Item { check { true || false && false; } }
        "#,
    );
    let mut builder = CfdDataModel::builder(&logical);
    builder.add_record(
        "item_1",
        "Item",
        std::iter::empty::<(&str, LoadedValueDraft)>(),
    );
    let model = build_model(&logical, builder);
    run_model_checks(&model, &logical).expect("logical && should bind tighter than ||");

    let bitwise = compile_schema(
        r#"
            type Item { check { 1 | 2 & 0 == 0; } }
        "#,
    );
    let mut builder = CfdDataModel::builder(&bitwise);
    builder.add_record(
        "item_1",
        "Item",
        std::iter::empty::<(&str, LoadedValueDraft)>(),
    );
    let model = build_model(&bitwise, builder);
    run_model_checks(&model, &bitwise)
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
        std::iter::empty::<(&str, LoadedValueDraft)>(),
    );
    let model = build_model(&guarded, builder);
    run_model_checks(&model, &guarded).expect("guarded check should pass");

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
        std::iter::empty::<(&str, LoadedValueDraft)>(),
    );
    let model = build_model(&unguarded, builder);
    let err = run_model_checks(&model, &unguarded).expect_err("null access");
    assert_has_code(&err, CfdErrorCode::CheckNullAccess);
}

#[test]
fn nullable_element_builtins_handle_nulls_and_empty_values() {
    let pass = compile_schema(
        r#"
            type Holder {
                nums: [int?] = [];
                check {
                    nums.isUnique();
                    nums.min() == 1;
                    nums.max() == 3;
                    nums.sum() == 4;
                    nums.contains(null);
                    nums.len() == 3;
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
            LoadedValueDraft::Array(vec![
                LoadedValueDraft::from(1_i64),
                LoadedValueDraft::Null,
                LoadedValueDraft::from(3_i64),
            ]),
        )],
    );
    let model = build_model(&pass, builder);
    run_model_checks(&model, &pass).expect("checks should pass");

    let empty = compile_schema(
        r#"
            type Holder {
                nums: [int?] = [];
                check { nums.min() >= 0; }
            }
        "#,
    );
    let mut builder = CfdDataModel::builder(&empty);
    builder.add_record(
        "holder_1",
        "Holder",
        [(
            "nums",
            LoadedValueDraft::Array(vec![LoadedValueDraft::Null]),
        )],
    );
    let model = build_model(&empty, builder);
    let err = run_model_checks(&model, &empty).expect_err("min over all-null values");
    assert_has_code(&err, CfdErrorCode::CheckEmptyMinMax);
}

#[test]
fn contains_reports_runtime_type_errors_for_null_collections() {
    let schema = compile_schema(
        r#"
            type Holder {
                items: [int]? = null;
                check { items.contains(1); }
            }
        "#,
    );

    let mut valid_builder = CfdDataModel::builder(&schema);
    valid_builder.add_record(
        "holder_valid",
        "Holder",
        [(
            "items",
            LoadedValueDraft::Array(vec![LoadedValueDraft::from(1_i64)]),
        )],
    );
    let valid = build_model(&schema, valid_builder);
    run_model_checks(&valid, &schema).expect("contains should work for a present nullable array");

    let mut null_builder = CfdDataModel::builder(&schema);
    null_builder.add_record(
        "holder_null",
        "Holder",
        std::iter::empty::<(&str, LoadedValueDraft)>(),
    );
    let null = build_model(&schema, null_builder);
    let err = run_model_checks(&null, &schema)
        .expect_err("contains(null, value) should be a runtime type error");

    assert_has_code(&err, CfdErrorCode::CheckEvalTypeError);
    assert!(
        !err.diagnostics.iter().any(|diagnostic| {
            matches!(
                diagnostic.code,
                CfdErrorCode::CheckFailed
                    | CfdErrorCode::CheckContainsFailed
                    | CfdErrorCode::CheckBoolExpectedTrue
            )
        }),
        "contains(null, value) must not be downgraded into a false check: {err:?}"
    );
}

#[test]
fn non_finite_float_comparisons_are_runtime_type_errors() {
    let schema = compile_schema(
        r#"
            type Holder {
                value: float;
                check {
                    value / value > 0.0;
                    ((0.0 - 1.0) ** 0.5) > 0.0;
                }
            }
        "#,
    );
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "holder_1",
        "Holder",
        [("value", LoadedValueDraft::from(0.0_f64))],
    );
    let model = build_model(&schema, builder);
    let err = run_model_checks(&model, &schema)
        .expect_err("NaN comparisons should fail as runtime type errors");

    assert_has_code(&err, CfdErrorCode::CheckEvalTypeError);
    assert!(
        !err.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == CfdErrorCode::CheckComparisonFailed),
        "NaN comparisons must not be downgraded into false comparisons: {err:?}"
    );
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
            ("first", LoadedValueDraft::from(0_i64)),
            ("second", LoadedValueDraft::from(0_i64)),
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
            ("first", LoadedValueDraft::from(0_i64)),
            ("second", LoadedValueDraft::from(0_i64)),
        ],
    );
    let model = build_model(&schema, builder);
    let err = run_model_checks(&model, &schema).expect_err("child checks fail");
    let paths = err
        .diagnostics
        .iter()
        .filter(|diag| diag.code == CfdErrorCode::CheckComparisonFailed)
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
            ("xs", LoadedValueDraft::Array(Vec::new())),
            ("value", LoadedValueDraft::from(0_i64)),
        ],
    );
    let model = build_model(&schema, builder);
    let err = run_model_checks(&model, &schema).expect_err("checks should fail");

    assert_has_code(&err, CfdErrorCode::CheckIndexOutOfBounds);
    assert_has_code(&err, CfdErrorCode::CheckComparisonFailed);
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
            LoadedValueDraft::Array(vec![
                LoadedValueDraft::from(-1_i64),
                LoadedValueDraft::from(-2_i64),
            ]),
        )],
    );
    let model = build_model(&soft_fail, builder);
    let err = run_model_checks(&model, &soft_fail).expect_err("all reports each failing element");
    let soft_fail_paths = err
        .diagnostics
        .iter()
        .filter(|diag| diag.code == CfdErrorCode::CheckAllQuantifierFailed)
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
            LoadedValueDraft::Array(vec![
                LoadedValueDraft::Array(Vec::new()),
                LoadedValueDraft::Array(vec![LoadedValueDraft::from(1_i64)]),
            ]),
        )],
    );
    let model = build_model(&hard_stop, builder);
    let err =
        run_model_checks(&model, &hard_stop).expect_err("hard eval error should not be swallowed");
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
            LoadedValueDraft::object_with_declared_type([("hp", LoadedValueDraft::from(0_i64))]),
        )],
    );
    let model = build_model(&schema, builder);
    let err = run_model_checks(&model, &schema).expect_err("nested check fails");
    assert_has_code(&err, CfdErrorCode::CheckComparisonFailed);
    let diag = err
        .diagnostics
        .iter()
        .find(|diag| diag.code == CfdErrorCode::CheckComparisonFailed)
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
            (
                "granted",
                LoadedValueDraft::enum_variant("Permission", "Read"),
            ),
            ("value", LoadedValueDraft::from(7_i64)),
        ],
    );
    let model = build_model(&schema, builder);
    run_model_checks(&model, &schema).expect("operators should pass");
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
            LoadedValueDraft::Array(vec![LoadedValueDraft::from(1_i64)]),
        )],
    );
    let model = build_model(&negative_index, builder);
    let err = run_model_checks(&model, &negative_index).expect_err("negative index should fail");
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
            LoadedValueDraft::dict([(
                LoadedDictKeyDraft::from("present"),
                LoadedValueDraft::from(1_i64),
            )]),
        )],
    );
    let model = build_model(&missing_key, builder);
    let err = run_model_checks(&model, &missing_key).expect_err("missing dict key should fail");
    assert_has_code(&err, CfdErrorCode::CheckMissingDictKey);

    let regex = compile_schema(
        r#"
            type Item {
                label: string;
                check { label.matches("配置"); }
            }
        "#,
    );
    let mut builder = CfdDataModel::builder(&regex);
    builder.add_record(
        "item_1",
        "Item",
        [("label", LoadedValueDraft::from("怪物配置"))],
    );
    let model = build_model(&regex, builder);
    run_model_checks(&model, &regex).expect("matches should use Unicode regex semantics");
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
                first: &Target;
                second: &Target;
                check { first.id == second.id; }
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "target_1",
        "Target",
        [("value", LoadedValueDraft::from(0_i64))],
    );
    builder.add_record(
        "holder_1",
        "Holder",
        [
            ("first", LoadedValueDraft::record_ref("target_1")),
            ("second", LoadedValueDraft::record_ref("target_1")),
        ],
    );
    let model = build_model(&schema, builder);
    let err = run_model_checks(&model, &schema).expect_err("invalid target should fail once");

    let failures = err
        .diagnostics
        .iter()
        .filter(|diag| diag.code == CfdErrorCode::CheckComparisonFailed)
        .collect::<Vec<_>>();
    assert_eq!(failures.len(), 1);
    assert_eq!(
        failures[0].primary.as_ref().and_then(|label| label.record),
        Some(record_id_at(&model, 0))
    );
}

#[test]
fn checks_through_refs_blame_the_target_value_and_relate_the_ref_source() {
    let schema = compile_schema(
        r#"
            type Target { price: int; }
            type Holder {
                item: &Target;
                check { item.price > 0; }
            }
        "#,
    );
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "target",
        "Target",
        [("price", LoadedValueDraft::from(0_i64))],
    );
    builder.add_record(
        "holder",
        "Holder",
        [("item", LoadedValueDraft::record_ref("target"))],
    );
    let model = build_model(&schema, builder);

    let err =
        run_model_checks(&model, &schema).expect_err("target price should fail the holder check");
    let diagnostic = err
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == CfdErrorCode::CheckComparisonFailed)
        .expect("comparison diagnostic");

    let primary = diagnostic.primary.as_ref().expect("primary location");
    assert_eq!(primary.record, Some(record_id_at(&model, 0)));
    assert_eq!(primary.path, CfdPath::root().field("price"));
    assert_eq!(diagnostic.related.len(), 1);
    assert_eq!(diagnostic.related[0].record, Some(record_id_at(&model, 1)));
    assert_eq!(diagnostic.related[0].path, CfdPath::root().field("item"));
}

#[test]
fn checks_preserve_every_hop_in_a_reference_chain() {
    let schema = compile_schema(
        r#"
            type Leaf { value: int; }
            type Middle { leaf: &Leaf; }
            type Root {
                middle: &Middle;
                check { middle.leaf.value > 0; }
            }
        "#,
    );
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("leaf", "Leaf", [("value", LoadedValueDraft::from(0_i64))]);
    builder.add_record(
        "middle",
        "Middle",
        [("leaf", LoadedValueDraft::record_ref("leaf"))],
    );
    builder.add_record(
        "root",
        "Root",
        [("middle", LoadedValueDraft::record_ref("middle"))],
    );
    let model = build_model(&schema, builder);

    let err = run_model_checks(&model, &schema).expect_err("leaf value should fail the root check");
    let diagnostic = err
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == CfdErrorCode::CheckComparisonFailed)
        .expect("comparison diagnostic");

    let primary = diagnostic.primary.as_ref().expect("primary location");
    assert_eq!(primary.record, Some(record_id_at(&model, 0)));
    assert_eq!(primary.path, CfdPath::root().field("value"));
    assert_eq!(diagnostic.related.len(), 2);
    assert_eq!(diagnostic.related[0].record, Some(record_id_at(&model, 2)));
    assert_eq!(diagnostic.related[0].path, CfdPath::root().field("middle"));
    assert_eq!(diagnostic.related[1].record, Some(record_id_at(&model, 1)));
    assert_eq!(diagnostic.related[1].path, CfdPath::root().field("leaf"));
}

#[test]
fn checks_keep_target_locations_through_collection_access_and_virtual_ids() {
    let schema = compile_schema(
        r#"
            type Target { nums: [int]; }
            type Holder {
                item: &Target;
                check {
                    item.nums[1] > 0;
                    item.id == "different";
                }
            }
        "#,
    );
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "target",
        "Target",
        [(
            "nums",
            LoadedValueDraft::Array(vec![
                LoadedValueDraft::from(1_i64),
                LoadedValueDraft::from(0_i64),
            ]),
        )],
    );
    builder.add_record(
        "holder",
        "Holder",
        [("item", LoadedValueDraft::record_ref("target"))],
    );
    let model = build_model(&schema, builder);

    let err =
        run_model_checks(&model, &schema).expect_err("target collection value and id should fail");
    let paths = err
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == CfdErrorCode::CheckComparisonFailed)
        .map(|diagnostic| {
            let primary = diagnostic.primary.as_ref().expect("primary location");
            assert_eq!(
                primary.record,
                Some(record_id_at(&model, 0)),
                "diagnostic: {diagnostic:?}"
            );
            assert_eq!(diagnostic.related.len(), 1);
            assert_eq!(diagnostic.related[0].record, Some(record_id_at(&model, 1)));
            assert_eq!(diagnostic.related[0].path, CfdPath::root().field("item"));
            primary.path.clone()
        })
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        paths,
        std::collections::BTreeSet::from([
            CfdPath::root().field("id"),
            CfdPath::root().field("nums").index(1),
        ])
    );
}

#[test]
fn checks_can_access_ref_fields_inherited_from_spread() {
    let schema = compile_schema(
        r#"
            type Item { price: int; }
            type Holder {
                item: &Item;
                check { item.price > 0; }
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("sword", "Item", [("price", LoadedValueDraft::from(1_i64))]);
    builder.add_record(
        "base",
        "Holder",
        [("item", LoadedValueDraft::record_ref("sword"))],
    );
    builder.add_loaded_record(LoadedRecordDraft::with_spreads(
        "copy",
        "Holder",
        [LoadedValueDraft::record_ref("base")],
        std::iter::empty::<(&str, LoadedValueDraft)>(),
    ));

    let model = build_model(&schema, builder);
    run_model_checks(&model, &schema).expect("spread-inherited ref should resolve in checks");

    let nested_schema = compile_schema(
        r#"
            type Item { price: int; }
            type Stats { item: &Item; }
            type Holder {
                stats: Stats;
                check { stats.item.price > 0; }
            }
        "#,
    );

    let mut nested_builder = CfdDataModel::builder(&nested_schema);
    nested_builder.add_record("sword", "Item", [("price", LoadedValueDraft::from(1_i64))]);
    nested_builder.add_record(
        "base_stats",
        "Stats",
        [("item", LoadedValueDraft::record_ref("sword"))],
    );
    nested_builder.add_record(
        "holder",
        "Holder",
        [(
            "stats",
            LoadedValueDraft::object_spread(
                [LoadedValueDraft::record_ref("base_stats")],
                std::iter::empty::<(&str, LoadedValueDraft)>(),
            ),
        )],
    );

    let nested_model = build_model(&nested_schema, nested_builder);
    run_model_checks(&nested_model, &nested_schema)
        .expect("nested spread-inherited ref should resolve in checks");

    let mut chained_builder = CfdDataModel::builder(&schema);
    chained_builder.add_record("sword", "Item", [("price", LoadedValueDraft::from(1_i64))]);
    chained_builder.add_record(
        "base",
        "Holder",
        [("item", LoadedValueDraft::record_ref("sword"))],
    );
    chained_builder.add_loaded_record(LoadedRecordDraft::with_spreads(
        "middle",
        "Holder",
        [LoadedValueDraft::record_ref("base")],
        std::iter::empty::<(&str, LoadedValueDraft)>(),
    ));
    chained_builder.add_loaded_record(LoadedRecordDraft::with_spreads(
        "copy",
        "Holder",
        [LoadedValueDraft::record_ref("middle")],
        std::iter::empty::<(&str, LoadedValueDraft)>(),
    ));

    let chained_model = build_model(&schema, chained_builder);
    run_model_checks(&chained_model, &schema)
        .expect("chained spread-inherited ref should resolve in checks");
}

#[test]
fn empty_sum_and_float_edge_semantics_are_preserved() {
    let empty_sum = compile_schema(
        r#"
            type Item {
                nums: [int] = [];
                check { nums.sum() == 0; }
            }
        "#,
    );
    let mut builder = CfdDataModel::builder(&empty_sum);
    builder.add_record(
        "item_1",
        "Item",
        std::iter::empty::<(&str, LoadedValueDraft)>(),
    );
    let model = build_model(&empty_sum, builder);
    run_model_checks(&model, &empty_sum).expect("empty int sum should evaluate as 0");

    let float_div_zero = compile_schema(
        r#"
            type Item {
                value: float;
                check { value / 0.0 > 0.0; }
            }
        "#,
    );
    let mut builder = CfdDataModel::builder(&float_div_zero);
    builder.add_record(
        "item_1",
        "Item",
        [("value", LoadedValueDraft::from(1.0_f64))],
    );
    let model = build_model(&float_div_zero, builder);
    run_model_checks(&model, &float_div_zero)
        .expect("float division by zero follows f64 infinity semantics");
}
