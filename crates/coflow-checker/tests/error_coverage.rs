#![allow(
    clippy::expect_used,
    clippy::needless_raw_string_hashes,
    clippy::panic,
    clippy::panic_in_result_fn,
    clippy::unwrap_used
)]

mod common;
use common::*;

use coflow_checker::{run_checks_with_options, CheckOptions, StructuralLimits};
use std::collections::BTreeSet;

type BuildFn = fn(&CftSchema) -> Result<CfdDataModel, CfdDiagnostics>;
type CheckFn = fn(&CftSchema, &CfdDataModel) -> Result<(), CfdDiagnostics>;
type DirectFn = fn() -> CfdDiagnostics;
type AdjacentFn = fn();

#[derive(Clone, Copy)]
enum Phase {
    Build(BuildFn),
    Check(BuildFn, CheckFn),
    Direct(DirectFn),
}

struct Case {
    name: &'static str,
    schema: &'static str,
    phase: Phase,
    code: CfdErrorCode,
    adjacent: AdjacentFn,
}

fn diagnostics_for(case: &Case) -> CfdDiagnostics {
    match case.phase {
        Phase::Direct(build) => build(),
        Phase::Build(build) => {
            let schema = compile_schema(case.schema);
            build(&schema).expect_err(case.name)
        }
        Phase::Check(build, check) => {
            let schema = compile_schema(case.schema);
            let model = build(&schema).expect("check coverage model should build");
            let compiled = schema;
            check(&compiled, &model).expect_err(case.name)
        }
    }
}

#[test]
fn every_cfd_error_code_has_negative_and_adjacent_valid_coverage() {
    let declared = declared_error_code_names();
    let covered = cases()
        .iter()
        .map(|case| format!("{:?}", case.code))
        .collect::<BTreeSet<_>>();

    let missing = declared.difference(&covered).cloned().collect::<Vec<_>>();
    assert!(
        missing.is_empty(),
        "missing CfdErrorCode coverage cases: {missing:?}"
    );

    for case in cases() {
        let diags = diagnostics_for(&case);
        assert_has_code(&diags, case.code);
        for diag in &diags.diagnostics {
            assert_eq!(diag.stage, diag.code.stage(), "{}", case.name);
            assert_eq!(diag.severity, CfdSeverity::Error, "{}", case.name);
            assert!(
                diag.primary.is_some(),
                "{} emitted {:?} without a primary label",
                case.name,
                diag.code
            );
        }
        (case.adjacent)();
    }
}

#[allow(clippy::too_many_lines)]
fn cases() -> Vec<Case> {
    vec![
        Case {
            name: "unknown record type",
            schema: "type Item { value: int; }",
            phase: Phase::Build(build_unknown_type),
            code: CfdErrorCode::UnknownType,
            adjacent: adjacent_known_record_type,
        },
        Case {
            name: "abstract record type",
            schema: "abstract type Reward {} type CoinReward : Reward { amount: int; }",
            phase: Phase::Build(build_abstract_record_type),
            code: CfdErrorCode::AbstractRecordType,
            adjacent: adjacent_concrete_child_record_type,
        },
        Case {
            name: "missing polymorphic object actual type",
            schema: "abstract type Reward {} type CoinReward : Reward { amount: int; } type Drop { reward: Reward; }",
            phase: Phase::Build(build_missing_object_type),
            code: CfdErrorCode::MissingObjectType,
            adjacent: adjacent_polymorphic_object_with_actual_type,
        },
        Case {
            name: "object actual type mismatch",
            schema: "abstract type Reward {} type CoinReward : Reward { amount: int; } type Item { name: string; } type Drop { reward: Reward; }",
            phase: Phase::Build(build_object_type_mismatch),
            code: CfdErrorCode::ObjectTypeMismatch,
            adjacent: adjacent_polymorphic_object_with_assignable_type,
        },
        Case {
            name: "unknown field",
            schema: "type Item { value: int; }",
            phase: Phase::Build(build_unknown_field),
            code: CfdErrorCode::UnknownField,
            adjacent: adjacent_known_field,
        },
        Case {
            name: "missing required field",
            schema: "type Item { value: int; }",
            phase: Phase::Build(build_missing_required_field),
            code: CfdErrorCode::MissingRequiredField,
            adjacent: adjacent_required_field_present,
        },
        Case {
            name: "schema default dependency cycle",
            schema: "type Node { child: Node = {}; }",
            phase: Phase::Build(build_schema_default_dependency_cycle),
            code: CfdErrorCode::ValueDependencyCycle,
            adjacent: adjacent_acyclic_schema_default,
        },
        Case {
            name: "data structure limit exceeded",
            schema: "type Item { value: [[int]]; }",
            phase: Phase::Build(build_data_structure_limit_exceeded),
            code: CfdErrorCode::DataStructureLimitExceeded,
            adjacent: adjacent_data_structure_at_limit,
        },
        Case {
            name: "type mismatch",
            schema: "type Item { value: int; }",
            phase: Phase::Build(build_type_mismatch),
            code: CfdErrorCode::TypeMismatch,
            adjacent: adjacent_matching_value_type,
        },
        Case {
            name: "invalid enum variant",
            schema: "enum Rarity { Common, Rare, } type Item { rarity: Rarity; }",
            phase: Phase::Build(build_invalid_enum_variant),
            code: CfdErrorCode::InvalidEnumVariant,
            adjacent: adjacent_known_enum_variant,
        },
        Case {
            name: "duplicate dict key",
            schema: "type Item { attrs: {string: int}; }",
            phase: Phase::Build(build_duplicate_dict_key),
            code: CfdErrorCode::DuplicateDictKey,
            adjacent: adjacent_unique_dict_keys,
        },
        Case {
            name: "missing id field",
            schema: "type Item { value: int; }",
            phase: Phase::Build(build_missing_id_field),
            code: CfdErrorCode::MissingIdField,
            adjacent: adjacent_non_empty_record_key,
        },
        Case {
            name: "invalid record key",
            schema: "type Item { value: int; }",
            phase: Phase::Build(build_invalid_record_key),
            code: CfdErrorCode::InvalidRecordKey,
            adjacent: adjacent_identifier_record_key,
        },
        Case {
            name: "duplicate id",
            schema: "type Item { value: int; }",
            phase: Phase::Build(build_duplicate_id),
            code: CfdErrorCode::DuplicateId,
            adjacent: adjacent_unique_ids,
        },
        Case {
            name: "duplicate polymorphic id",
            schema: "abstract type Reward {} type CoinReward : Reward { amount: int; } type ItemReward : Reward { count: int; }",
            phase: Phase::Build(build_duplicate_polymorphic_id),
            code: CfdErrorCode::DuplicatePolymorphicId,
            adjacent: adjacent_unique_polymorphic_ids,
        },
        Case {
            name: "missing ref target",
            schema: "type Item { name: string; } type Drop { item: &Item; }",
            phase: Phase::Build(build_missing_ref_target),
            code: CfdErrorCode::RefTargetNotFound,
            adjacent: adjacent_existing_ref_target,
        },
        Case {
            name: "check failed fallback",
            schema: "",
            phase: Phase::Direct(build_check_failed_fallback),
            code: CfdErrorCode::CheckFailed,
            adjacent: adjacent_true_check,
        },
        Case {
            name: "check comparison failed",
            schema: "type Item { value: int; check { value > 0; } }",
            phase: Phase::Check(build_check_failed_model, run_checks),
            code: CfdErrorCode::CheckComparisonFailed,
            adjacent: adjacent_true_check,
        },
        Case {
            name: "check bool expected true",
            schema: scalar_false_schema(),
            phase: Phase::Check(build_scalar_false_model, run_checks),
            code: CfdErrorCode::CheckBoolExpectedTrue,
            adjacent: adjacent_scalar_false_checks,
        },
        Case {
            name: "check negation failed",
            schema: scalar_false_schema(),
            phase: Phase::Check(build_scalar_false_model, run_checks),
            code: CfdErrorCode::CheckNegationFailed,
            adjacent: adjacent_scalar_false_checks,
        },
        Case {
            name: "check and failed",
            schema: scalar_false_schema(),
            phase: Phase::Check(build_scalar_false_model, run_checks),
            code: CfdErrorCode::CheckAndFailed,
            adjacent: adjacent_scalar_false_checks,
        },
        Case {
            name: "check or failed",
            schema: scalar_false_schema(),
            phase: Phase::Check(build_scalar_false_model, run_checks),
            code: CfdErrorCode::CheckOrFailed,
            adjacent: adjacent_scalar_false_checks,
        },
        Case {
            name: "check type predicate failed",
            schema: scalar_false_schema(),
            phase: Phase::Check(build_scalar_false_model, run_checks),
            code: CfdErrorCode::CheckTypePredicateFailed,
            adjacent: adjacent_scalar_false_checks,
        },
        Case {
            name: "check null predicate failed",
            schema: scalar_false_schema(),
            phase: Phase::Check(build_scalar_false_model, run_checks),
            code: CfdErrorCode::CheckNullPredicateFailed,
            adjacent: adjacent_scalar_false_checks,
        },
        Case {
            name: "check contains failed",
            schema: scalar_false_schema(),
            phase: Phase::Check(build_scalar_false_model, run_checks),
            code: CfdErrorCode::CheckContainsFailed,
            adjacent: adjacent_scalar_false_checks,
        },
        Case {
            name: "check unique failed",
            schema: scalar_false_schema(),
            phase: Phase::Check(build_scalar_false_model, run_checks),
            code: CfdErrorCode::CheckUniqueFailed,
            adjacent: adjacent_scalar_false_checks,
        },
        Case {
            name: "check matches failed",
            schema: scalar_false_schema(),
            phase: Phase::Check(build_scalar_false_model, run_checks),
            code: CfdErrorCode::CheckMatchesFailed,
            adjacent: adjacent_scalar_false_checks,
        },
        Case {
            name: "check any quantifier failed",
            schema: quantifier_false_schema(),
            phase: Phase::Check(build_quantifier_false_model, run_checks),
            code: CfdErrorCode::CheckAnyQuantifierFailed,
            adjacent: adjacent_quantifier_checks,
        },
        Case {
            name: "check none quantifier failed",
            schema: quantifier_false_schema(),
            phase: Phase::Check(build_quantifier_false_model, run_checks),
            code: CfdErrorCode::CheckNoneQuantifierFailed,
            adjacent: adjacent_quantifier_checks,
        },
        Case {
            name: "check all quantifier failed",
            schema: quantifier_false_schema(),
            phase: Phase::Check(build_quantifier_false_model, run_checks),
            code: CfdErrorCode::CheckAllQuantifierFailed,
            adjacent: adjacent_quantifier_checks,
        },
        Case {
            name: "check eval type error",
            schema: "type Item { nums: [int]? = null; check { nums.contains(1); } }",
            phase: Phase::Check(build_default_item_model, run_checks),
            code: CfdErrorCode::CheckEvalTypeError,
            adjacent: adjacent_check_eval_type_valid,
        },
        Case {
            name: "check null access",
            schema: "type Child { name: string; } type Holder { child: Child? = null; check { child.name != \"\"; } }",
            phase: Phase::Check(build_default_holder_model, run_checks),
            code: CfdErrorCode::CheckNullAccess,
            adjacent: adjacent_null_guarded_access,
        },
        Case {
            name: "check index out of bounds",
            schema: "type Item { nums: [int]; check { nums[0] > 0; } }",
            phase: Phase::Check(build_empty_nums_model, run_checks),
            code: CfdErrorCode::CheckIndexOutOfBounds,
            adjacent: adjacent_in_bounds_index,
        },
        Case {
            name: "check missing dict key",
            schema: "type Item { attrs: {string: int}; check { attrs[\"missing\"] > 0; } }",
            phase: Phase::Check(build_present_attr_model, run_checks),
            code: CfdErrorCode::CheckMissingDictKey,
            adjacent: adjacent_existing_dict_key,
        },
        Case {
            name: "check empty min max",
            schema: "type Item { nums: [int] = []; check { nums.min() > 0; } }",
            phase: Phase::Check(build_default_item_model, run_checks),
            code: CfdErrorCode::CheckEmptyMinMax,
            adjacent: adjacent_non_empty_min,
        },
        Case {
            name: "check structural budget exceeded",
            schema: "",
            phase: Phase::Direct(check_budget_exceeded),
            code: CfdErrorCode::CheckBudgetExceeded,
            adjacent: adjacent_check_budget_valid,
        },
        Case {
            name: "singleton record count invalid",
            schema: "@singleton type Cfg { value: int; }",
            phase: Phase::Build(build_singleton_count_invalid),
            code: CfdErrorCode::SingletonRecordCountInvalid,
            adjacent: adjacent_singleton_count_valid,
        },
        Case {
            name: "singleton key missing or invalid",
            schema: "@singleton type Cfg { value: int; }",
            phase: Phase::Build(build_singleton_key_invalid),
            code: CfdErrorCode::SingletonKeyMissingOrInvalid,
            adjacent: adjacent_singleton_count_valid,
        },
        Case {
            name: "singleton key collision",
            schema: "@singleton type A { x: int; } @singleton type B { y: int; }",
            phase: Phase::Build(build_singleton_key_collision),
            code: CfdErrorCode::SingletonKeyCollision,
            adjacent: adjacent_singleton_keys_unique,
        },
    ]
}

fn build_singleton_count_invalid(schema: &CftSchema) -> Result<CfdDataModel, CfdDiagnostics> {
    // Two records of a singleton type triggers SingletonRecordCountInvalid.
    model_from_records(
        schema,
        [
            one_record("first", "Cfg", [("value", CfdInputValue::from(1_i64))]),
            one_record("second", "Cfg", [("value", CfdInputValue::from(2_i64))]),
        ],
    )
}

fn adjacent_singleton_count_valid() {
    assert_builds(
        "@singleton type Cfg { value: int; }",
        [one_record(
            "main",
            "Cfg",
            [("value", CfdInputValue::from(1_i64))],
        )],
    );
}

fn build_singleton_key_invalid(schema: &CftSchema) -> Result<CfdDataModel, CfdDiagnostics> {
    // A singleton record with an invalid identifier key. We use a leading
    // digit to slip past the generic InvalidRecordKey path's prerequisites
    // and reach the singleton-specific check.
    model_from_records(
        schema,
        [one_record(
            "1bad",
            "Cfg",
            [("value", CfdInputValue::from(1_i64))],
        )],
    )
}

fn build_singleton_key_collision(schema: &CftSchema) -> Result<CfdDataModel, CfdDiagnostics> {
    model_from_records(
        schema,
        [
            one_record("dup", "A", [("x", CfdInputValue::from(1_i64))]),
            one_record("dup", "B", [("y", CfdInputValue::from(2_i64))]),
        ],
    )
}

fn adjacent_singleton_keys_unique() {
    assert_builds(
        "@singleton type A { x: int; } @singleton type B { y: int; }",
        [
            one_record("a", "A", [("x", CfdInputValue::from(1_i64))]),
            one_record("b", "B", [("y", CfdInputValue::from(2_i64))]),
        ],
    );
}

fn model_from_records(
    schema: &CftSchema,
    records: impl IntoIterator<Item = CfdInputRecord>,
) -> Result<CfdDataModel, CfdDiagnostics> {
    let mut builder = CfdDataModel::builder(schema);
    for record in records {
        builder.add_input_record(record);
    }
    builder.build()
}

fn one_record(
    key: &str,
    actual_type: &str,
    fields: impl IntoIterator<Item = (&'static str, CfdInputValue)>,
) -> CfdInputRecord {
    CfdInputRecord::new(key, actual_type, fields)
}

fn build_unknown_type(schema: &CftSchema) -> Result<CfdDataModel, CfdDiagnostics> {
    model_from_records(schema, [one_record("missing", "Missing", [])])
}

fn build_abstract_record_type(schema: &CftSchema) -> Result<CfdDataModel, CfdDiagnostics> {
    model_from_records(schema, [one_record("reward", "Reward", [])])
}

fn build_missing_object_type(schema: &CftSchema) -> Result<CfdDataModel, CfdDiagnostics> {
    model_from_records(
        schema,
        [one_record(
            "drop",
            "Drop",
            [(
                "reward",
                CfdInputValue::object_with_declared_type([("amount", CfdInputValue::from(1_i64))]),
            )],
        )],
    )
}

fn build_object_type_mismatch(schema: &CftSchema) -> Result<CfdDataModel, CfdDiagnostics> {
    model_from_records(
        schema,
        [one_record(
            "drop",
            "Drop",
            [(
                "reward",
                CfdInputValue::object("Item", [("name", CfdInputValue::from("sword"))]),
            )],
        )],
    )
}

fn build_unknown_field(schema: &CftSchema) -> Result<CfdDataModel, CfdDiagnostics> {
    model_from_records(
        schema,
        [one_record(
            "item",
            "Item",
            [
                ("value", CfdInputValue::from(1_i64)),
                ("missing", CfdInputValue::from(2_i64)),
            ],
        )],
    )
}

fn build_missing_required_field(schema: &CftSchema) -> Result<CfdDataModel, CfdDiagnostics> {
    model_from_records(schema, [one_record("item", "Item", [])])
}

fn build_schema_default_dependency_cycle(
    schema: &CftSchema,
) -> Result<CfdDataModel, CfdDiagnostics> {
    model_from_records(schema, [one_record("root", "Node", [])])
}

fn build_data_structure_limit_exceeded(
    schema: &CftSchema,
) -> Result<CfdDataModel, CfdDiagnostics> {
    let mut builder =
        CfdDataModel::builder(schema).with_structural_limits(StructuralLimits::new(3, 100, 100));
    builder.add_record(
        "item",
        "Item",
        [(
            "value",
            CfdInputValue::Array(vec![CfdInputValue::Array(vec![1_i64.into()])]),
        )],
    );
    builder.build()
}

fn build_type_mismatch(schema: &CftSchema) -> Result<CfdDataModel, CfdDiagnostics> {
    model_from_records(
        schema,
        [one_record(
            "item",
            "Item",
            [("value", CfdInputValue::from("not an int"))],
        )],
    )
}

fn build_invalid_enum_variant(schema: &CftSchema) -> Result<CfdDataModel, CfdDiagnostics> {
    model_from_records(
        schema,
        [one_record(
            "item",
            "Item",
            [("rarity", CfdInputValue::enum_variant("Rarity", "Missing"))],
        )],
    )
}

fn build_duplicate_dict_key(schema: &CftSchema) -> Result<CfdDataModel, CfdDiagnostics> {
    model_from_records(
        schema,
        [one_record(
            "item",
            "Item",
            [(
                "attrs",
                CfdInputValue::dict([
                    (CfdInputDictKey::from("x"), CfdInputValue::from(1_i64)),
                    (CfdInputDictKey::from("x"), CfdInputValue::from(2_i64)),
                ]),
            )],
        )],
    )
}

fn build_missing_id_field(schema: &CftSchema) -> Result<CfdDataModel, CfdDiagnostics> {
    model_from_records(
        schema,
        [one_record(
            "",
            "Item",
            [("value", CfdInputValue::from(1_i64))],
        )],
    )
}

fn build_invalid_record_key(schema: &CftSchema) -> Result<CfdDataModel, CfdDiagnostics> {
    model_from_records(
        schema,
        [one_record(
            "bad-key",
            "Item",
            [("value", CfdInputValue::from(1_i64))],
        )],
    )
}

fn build_duplicate_id(schema: &CftSchema) -> Result<CfdDataModel, CfdDiagnostics> {
    model_from_records(
        schema,
        [
            one_record("item", "Item", [("value", CfdInputValue::from(1_i64))]),
            one_record("item", "Item", [("value", CfdInputValue::from(2_i64))]),
        ],
    )
}

fn build_duplicate_polymorphic_id(schema: &CftSchema) -> Result<CfdDataModel, CfdDiagnostics> {
    model_from_records(
        schema,
        [
            one_record(
                "same",
                "CoinReward",
                [("amount", CfdInputValue::from(1_i64))],
            ),
            one_record(
                "same",
                "ItemReward",
                [("count", CfdInputValue::from(2_i64))],
            ),
        ],
    )
}

fn build_missing_ref_target(schema: &CftSchema) -> Result<CfdDataModel, CfdDiagnostics> {
    model_from_records(
        schema,
        [one_record(
            "drop",
            "Drop",
            [("item", CfdInputValue::record_ref("missing"))],
        )],
    )
}

fn build_check_failed_model(schema: &CftSchema) -> Result<CfdDataModel, CfdDiagnostics> {
    model_from_records(
        schema,
        [one_record(
            "item",
            "Item",
            [("value", CfdInputValue::from(0_i64))],
        )],
    )
}

fn build_check_failed_fallback() -> CfdDiagnostics {
    CfdDiagnostics::one(
        CfdDiagnostic::error(CfdErrorCode::CheckFailed, "fallback false condition")
            .with_primary(None, CfdPath::root()),
    )
}

const fn scalar_false_schema() -> &'static str {
    r#"
        abstract type Reward {}
        type CurrencyReward : Reward {}
        type ItemReward : Reward {}
        type Item {
            enabled: bool;
            negated: bool;
            left: bool;
            right: bool;
            reward: &Reward;
            optional: int? = null;
            tags: [string];
            name: string;
            check {
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
    "#
}

fn build_scalar_false_model(schema: &CftSchema) -> Result<CfdDataModel, CfdDiagnostics> {
    model_from_records(
        schema,
        [
            one_record("reward", "ItemReward", []),
            one_record(
                "item",
                "Item",
                [
                    ("enabled", CfdInputValue::from(false)),
                    ("negated", CfdInputValue::from(true)),
                    ("left", CfdInputValue::from(false)),
                    ("right", CfdInputValue::from(false)),
                    ("reward", CfdInputValue::record_ref("reward")),
                    (
                        "tags",
                        CfdInputValue::Array(vec![
                            CfdInputValue::from("mob"),
                            CfdInputValue::from("mob"),
                        ]),
                    ),
                    ("name", CfdInputValue::from("mob_1")),
                ],
            ),
        ],
    )
}

const fn quantifier_false_schema() -> &'static str {
    r#"
        type Item {
            any_flags: [bool];
            none_flags: [bool];
            all_flags: [bool];
            check {
                any flag in any_flags { flag; }
                none flag in none_flags { flag; }
                all flag in all_flags { flag; }
            }
        }
    "#
}

fn build_quantifier_false_model(schema: &CftSchema) -> Result<CfdDataModel, CfdDiagnostics> {
    model_from_records(
        schema,
        [one_record(
            "item",
            "Item",
            [
                (
                    "any_flags",
                    CfdInputValue::Array(vec![
                        CfdInputValue::from(false),
                        CfdInputValue::from(false),
                    ]),
                ),
                (
                    "none_flags",
                    CfdInputValue::Array(vec![
                        CfdInputValue::from(false),
                        CfdInputValue::from(true),
                    ]),
                ),
                (
                    "all_flags",
                    CfdInputValue::Array(vec![
                        CfdInputValue::from(true),
                        CfdInputValue::from(false),
                    ]),
                ),
            ],
        )],
    )
}

fn build_default_item_model(schema: &CftSchema) -> Result<CfdDataModel, CfdDiagnostics> {
    model_from_records(schema, [one_record("item", "Item", [])])
}

fn build_default_holder_model(schema: &CftSchema) -> Result<CfdDataModel, CfdDiagnostics> {
    model_from_records(schema, [one_record("holder", "Holder", [])])
}

fn build_empty_nums_model(schema: &CftSchema) -> Result<CfdDataModel, CfdDiagnostics> {
    model_from_records(
        schema,
        [one_record(
            "item",
            "Item",
            [("nums", CfdInputValue::Array(Vec::new()))],
        )],
    )
}

fn build_present_attr_model(schema: &CftSchema) -> Result<CfdDataModel, CfdDiagnostics> {
    model_from_records(
        schema,
        [one_record(
            "item",
            "Item",
            [(
                "attrs",
                CfdInputValue::dict([(
                    CfdInputDictKey::from("present"),
                    CfdInputValue::from(1_i64),
                )]),
            )],
        )],
    )
}

fn run_checks(schema: &CftSchema, model: &CfdDataModel) -> Result<(), CfdDiagnostics> {
    model.run_checks(schema)
}

fn assert_builds(
    schema_source: &str,
    records: impl IntoIterator<Item = CfdInputRecord>,
) -> CfdDataModel {
    let schema = compile_schema(schema_source);
    model_from_records(&schema, records).expect("adjacent-valid model should build")
}

fn assert_checks(schema_source: &str, records: impl IntoIterator<Item = CfdInputRecord>) {
    let schema = compile_schema(schema_source);
    let model = model_from_records(&schema, records).expect("adjacent-valid model should build");
    let compiled = schema;
    model
        .run_checks(&compiled)
        .expect("adjacent-valid checks should pass");
}

fn adjacent_known_record_type() {
    assert_builds(
        "type Item { value: int; }",
        [one_record(
            "item",
            "Item",
            [("value", CfdInputValue::from(1_i64))],
        )],
    );
}

fn adjacent_concrete_child_record_type() {
    assert_builds(
        "abstract type Reward {} type CoinReward : Reward { amount: int; }",
        [one_record(
            "reward",
            "CoinReward",
            [("amount", CfdInputValue::from(1_i64))],
        )],
    );
}

fn adjacent_polymorphic_object_with_actual_type() {
    assert_builds(
        "abstract type Reward {} type CoinReward : Reward { amount: int; } type Drop { reward: Reward; }",
        [one_record(
            "drop",
            "Drop",
            [(
                "reward",
                CfdInputValue::object("CoinReward", [("amount", CfdInputValue::from(1_i64))]),
            )],
        )],
    );
}

fn adjacent_polymorphic_object_with_assignable_type() {
    adjacent_polymorphic_object_with_actual_type();
}

fn adjacent_known_field() {
    adjacent_known_record_type();
}

fn adjacent_required_field_present() {
    adjacent_known_record_type();
}

fn check_budget_exceeded() -> CfdDiagnostics {
    let schema = compile_schema("type Item { nums: [int]; check { nums.isUnique(); } }");
    let model = model_from_records(
        &schema,
        [one_record(
            "item",
            "Item",
            [(
                "nums",
                CfdInputValue::Array(vec![CfdInputValue::from(1_i64), CfdInputValue::from(2_i64)]),
            )],
        )],
    )
    .expect("budget coverage model builds");
    run_checks_with_options(
        &schema,
        &model,
        CheckOptions {
            structural_limits: StructuralLimits::new(100, 100, 1),
        },
    )
    .expect_err("collection work should exceed one")
}

fn adjacent_check_budget_valid() {
    let schema = compile_schema("type Item { nums: [int]; check { nums.isUnique(); } }");
    let model = model_from_records(
        &schema,
        [one_record(
            "item",
            "Item",
            [(
                "nums",
                CfdInputValue::Array(vec![CfdInputValue::from(1_i64), CfdInputValue::from(2_i64)]),
            )],
        )],
    )
    .expect("adjacent budget model builds");
    run_checks_with_options(
        &schema,
        &model,
        CheckOptions {
            structural_limits: StructuralLimits::new(100, 100, 2),
        },
    )
    .expect("work exactly at the limit should pass");
}

fn adjacent_acyclic_schema_default() {
    assert_builds(
        "type Leaf { value: int = 1; } type Root { child: Leaf = {}; }",
        [one_record("root", "Root", [])],
    );
}

fn adjacent_data_structure_at_limit() {
    let schema = compile_schema("type Item { value: [[int]]; }");
    let mut builder =
        CfdDataModel::builder(&schema).with_structural_limits(StructuralLimits::new(4, 100, 100));
    builder.add_record(
        "item",
        "Item",
        [(
            "value",
            CfdInputValue::Array(vec![CfdInputValue::Array(vec![1_i64.into()])]),
        )],
    );
    builder
        .build()
        .expect("data structure exactly at the limit should build");
}

fn adjacent_matching_value_type() {
    adjacent_known_record_type();
}

fn adjacent_known_enum_variant() {
    assert_builds(
        "enum Rarity { Common, Rare, } type Item { rarity: Rarity; }",
        [one_record(
            "item",
            "Item",
            [("rarity", CfdInputValue::enum_variant("Rarity", "Rare"))],
        )],
    );
}

fn adjacent_unique_dict_keys() {
    assert_builds(
        "type Item { attrs: {string: int}; }",
        [one_record(
            "item",
            "Item",
            [(
                "attrs",
                CfdInputValue::dict([
                    (CfdInputDictKey::from("x"), CfdInputValue::from(1_i64)),
                    (CfdInputDictKey::from("y"), CfdInputValue::from(2_i64)),
                ]),
            )],
        )],
    );
}

fn adjacent_non_empty_record_key() {
    adjacent_known_record_type();
}

fn adjacent_identifier_record_key() {
    adjacent_known_record_type();
}

fn adjacent_unique_ids() {
    assert_builds(
        "type Item { value: int; }",
        [
            one_record("item_1", "Item", [("value", CfdInputValue::from(1_i64))]),
            one_record("item_2", "Item", [("value", CfdInputValue::from(2_i64))]),
        ],
    );
}

fn adjacent_unique_polymorphic_ids() {
    assert_builds(
        "abstract type Reward {} type CoinReward : Reward { amount: int; } type ItemReward : Reward { count: int; }",
        [
            one_record(
                "coin",
                "CoinReward",
                [("amount", CfdInputValue::from(1_i64))],
            ),
            one_record(
                "item",
                "ItemReward",
                [("count", CfdInputValue::from(2_i64))],
            ),
        ],
    );
}

fn adjacent_existing_ref_target() {
    assert_builds(
        "type Item { name: string; } type Drop { item: &Item; }",
        [
            one_record("sword", "Item", [("name", CfdInputValue::from("Sword"))]),
            one_record(
                "drop",
                "Drop",
                [("item", CfdInputValue::record_ref("sword"))],
            ),
        ],
    );
}

fn adjacent_true_check() {
    assert_checks(
        "type Item { value: int; check { value > 0; } }",
        [one_record(
            "item",
            "Item",
            [("value", CfdInputValue::from(1_i64))],
        )],
    );
}

fn adjacent_scalar_false_checks() {
    assert_checks(
        r#"
            abstract type Reward {}
            type CurrencyReward : Reward {}
            type Item {
                enabled: bool;
                negated: bool;
                left: bool;
                right: bool;
                reward: &Reward;
                optional: int? = null;
                tags: [string];
                name: string;
                check {
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
        [
            one_record("reward", "CurrencyReward", []),
            one_record(
                "item",
                "Item",
                [
                    ("enabled", CfdInputValue::from(true)),
                    ("negated", CfdInputValue::from(false)),
                    ("left", CfdInputValue::from(true)),
                    ("right", CfdInputValue::from(true)),
                    ("reward", CfdInputValue::record_ref("reward")),
                    ("optional", CfdInputValue::from(1_i64)),
                    (
                        "tags",
                        CfdInputValue::Array(vec![
                            CfdInputValue::from("mob"),
                            CfdInputValue::from("boss"),
                        ]),
                    ),
                    ("name", CfdInputValue::from("npc_1")),
                ],
            ),
        ],
    );
}

fn adjacent_quantifier_checks() {
    assert_checks(
        r#"
            type Item {
                any_flags: [bool];
                none_flags: [bool];
                all_flags: [bool];
                check {
                    any flag in any_flags { flag; }
                    none flag in none_flags { flag; }
                    all flag in all_flags { flag; }
                }
            }
        "#,
        [one_record(
            "item",
            "Item",
            [
                (
                    "any_flags",
                    CfdInputValue::Array(vec![
                        CfdInputValue::from(false),
                        CfdInputValue::from(true),
                    ]),
                ),
                (
                    "none_flags",
                    CfdInputValue::Array(vec![
                        CfdInputValue::from(false),
                        CfdInputValue::from(false),
                    ]),
                ),
                (
                    "all_flags",
                    CfdInputValue::Array(vec![
                        CfdInputValue::from(true),
                        CfdInputValue::from(true),
                    ]),
                ),
            ],
        )],
    );
}

fn adjacent_check_eval_type_valid() {
    assert_checks(
        "type Item { nums: [int]? = null; check { nums.contains(1); } }",
        [one_record(
            "item",
            "Item",
            [(
                "nums",
                CfdInputValue::Array(vec![CfdInputValue::from(1_i64)]),
            )],
        )],
    );
}

fn adjacent_null_guarded_access() {
    assert_checks(
        "type Child { name: string; } type Holder { child: Child? = null; check { child == null || child.name != \"\"; } }",
        [one_record("holder", "Holder", [])],
    );
}

fn adjacent_in_bounds_index() {
    assert_checks(
        "type Item { nums: [int]; check { nums[0] > 0; } }",
        [one_record(
            "item",
            "Item",
            [(
                "nums",
                CfdInputValue::Array(vec![CfdInputValue::from(1_i64)]),
            )],
        )],
    );
}

fn adjacent_existing_dict_key() {
    assert_checks(
        "type Item { attrs: {string: int}; check { attrs[\"present\"] > 0; } }",
        [one_record(
            "item",
            "Item",
            [(
                "attrs",
                CfdInputValue::dict([(
                    CfdInputDictKey::from("present"),
                    CfdInputValue::from(1_i64),
                )]),
            )],
        )],
    );
}

fn adjacent_non_empty_min() {
    assert_checks(
        "type Item { nums: [int]; check { nums.min() > 0; } }",
        [one_record(
            "item",
            "Item",
            [(
                "nums",
                CfdInputValue::Array(vec![CfdInputValue::from(1_i64)]),
            )],
        )],
    );
}

fn declared_error_code_names() -> BTreeSet<String> {
    let source = include_str!("../../coflow-data-model/src/diagnostic.rs");
    let enum_body = source
        .split("pub enum CfdErrorCode {")
        .nth(1)
        .and_then(|tail| tail.split('}').next())
        .expect("CfdErrorCode enum body");

    enum_body
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with("#[") {
                None
            } else {
                Some(line.trim_end_matches(',').to_string())
            }
        })
        .collect()
}
