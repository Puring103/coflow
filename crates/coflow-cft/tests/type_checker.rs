#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::panic_in_result_fn,
    clippy::too_many_lines,
    clippy::needless_raw_string_hashes,
    clippy::doc_markdown
)]

mod common;
use common::*;

#[test]
fn type_checker_reports_name_field_enum_function_quantifier_index_and_regex_errors() {
    let source = r#"
        const PAT = "^[a";
        enum Rarity { Common, Rare, }
        type Item {
            key: string;
            count: int;
            rarity: Rarity;
            tags: [string];
            scores: {string: int};
            check {
                missing != "";
                key.missing != "";
                Rarity.Missing == rarity;
                rarity > 5;
                key.len();
                all ch in key { ch != ""; }
                tags["x"] != "";
                key.matches(PAT);
                key.matches("[");
            }
        }
    "#;
    let err = compile_one(source).unwrap_err();
    assert_has_code(&err, CftErrorCode::UnknownValueName);
    assert_has_code(&err, CftErrorCode::FieldAccessOnNonObject);
    assert_has_code(&err, CftErrorCode::TypeUnknownEnumVariant);
    assert_has_code(&err, CftErrorCode::ComparisonTypeMismatch);
    assert_has_code(&err, CftErrorCode::QuantifierRequiresCollection);
    assert_has_code(&err, CftErrorCode::IndexTypeMismatch);
    assert_has_code(&err, CftErrorCode::RegexPatternMustBeLiteral);
    assert_has_code(&err, CftErrorCode::InvalidRegexPattern);
}

#[test]
fn type_checker_rejects_reserved_quantifier_binding() {
    let source = r#"
        type Item {
            nums: [int];
            check {
                all module in nums { module > 0; }
            }
        }
    "#;

    let err = compile_one(source).unwrap_err();
    assert_has_code(&err, CftErrorCode::ReservedIdentifier);
}

#[test]
fn type_checker_accepts_nullable_guarded_access_and_object_fields() {
    let source = r#"
        type Item {
            key: string;
            rarity: int;
        }

        type Holder {
            maybe: Item? = null;
            item: Item;

            check {
                maybe != null && maybe.key != "";
                item.key != "";
                item.rarity >= 0;
            }
        }
    "#;

    compile_one(source).unwrap();
}

#[test]
fn type_checker_accepts_safe_access_and_rejects_invalid_nullable_operators() {
    compile_one(
        r#"
        type Item { value: int; }
        type Holder {
            maybe: Item? = null;
            numbers: [int]? = null;
            check {
                (maybe?.value ?? 0) >= 0;
                (numbers?[0] ?? 0) >= 0;
            }
        }
        "#,
    )
    .expect("safe access and matching fallback should compile");

    let error = compile_one(
        r#"
        type Item { value: int; }
        type Holder {
            item: Item;
            numbers: [int];
            maybe: int? = null;
            check {
                item?.value > 0;
                numbers?[0] > 0;
                (maybe ?? "wrong") > 0;
                (1 ?? 0) > 0;
            }
        }
        "#,
    )
    .unwrap_err();
    assert_has_code(&error, CftErrorCode::OperatorTypeMismatch);
    assert!(
        error
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == CftErrorCode::OperatorTypeMismatch)
            .count()
            >= 4,
        "{error:#?}"
    );
}

#[test]
fn type_checker_allows_is_null_for_nullable_operands() {
    compile_one(
        r#"
            type Child { key: string; }
            type Holder {
                maybe_int: int? = null;
                maybe_child: Child? = null;
                check {
                    maybe_int is null;
                    maybe_child is null;
                }
            }
        "#,
    )
    .expect("nullable operands may use is null");
}

#[test]
fn type_checker_accepts_nullable_element_builtins() {
    compile_one(
        r#"
            type Holder {
                nums: [int?] = [];
                check {
                    nums.isUnique();
                    nums.min() >= 0;
                    nums.max() >= 0;
                    nums.sum() >= 0;
                    nums.contains(null);
                }
            }
        "#,
    )
    .expect("nullable element arrays are supported by built-ins");
}

#[test]
fn type_checker_treats_nullable_element_min_max_results_as_non_null() {
    let err = compile_one(
        r#"
            type Holder {
                nums: [int?] = [];
                check {
                    nums.min() is null;
                    nums.max() is null;
                }
            }
        "#,
    )
    .expect_err("min/max over nullable elements should return non-null values");
    assert_has_code(&err, CftErrorCode::OperatorTypeMismatch);
}

#[test]
fn type_checker_rejects_is_null_for_non_nullable_operands() {
    let err = compile_one(
        r#"
            type Holder {
                value: int;
                check { value is null; }
            }
        "#,
    )
    .expect_err("non-nullable operand should fail");
    assert_has_code(&err, CftErrorCode::OperatorTypeMismatch);
}

#[test]
fn type_checker_reports_is_predicate_and_condition_edges() {
    let source = r#"
        enum E { A, }
        type Item {
            key: string;
            check {
                key is E;
                key;
                when key { true; }
            }
        }
    "#;

    let err = compile_one(source).unwrap_err();
    assert_has_code(&err, CftErrorCode::InvalidIsPredicate);
    assert_has_code(&err, CftErrorCode::ConditionMustBeBool);
}

#[test]
fn type_checker_reports_is_on_non_object_left_operand() {
    let source = r#"
        type Item {
            key: string;
            check {
                key is Item;
            }
        }
    "#;

    let err = compile_one(source).unwrap_err();
    assert_has_code(&err, CftErrorCode::OperatorTypeMismatch);
}

#[test]
fn type_checker_reports_unknown_field_on_known_object_operand() {
    let source = r#"
        type Child { key: string; }
        type Holder {
            child: Child;
            check {
                child.missing != "";
            }
        }
    "#;

    let err = compile_one(source).expect_err("unknown object field should fail");
    assert_has_code(&err, CftErrorCode::UnknownField);
}

#[test]
fn type_checker_reports_bitwise_shift_and_function_edges() {
    let source = r#"
        @flag
        enum Flags { A = 1, B = 2, }
        enum Color { Red, Blue, }

        type Item {
            flags: Flags;
            color: Color;
            numbers: [int];
            texts: [string];
            floats: [float];
            objects: [Item];

            check {
                flags | Flags.A != Flags.B;
                color | Color.Red != Color.Blue;
                flags & 1 != Flags.A;
                color << 1 == color;
                floats.isUnique();
                objects.isUnique();
                texts.min() != "";
                texts.sum() == 0;
                numbers.contains("x");
                numbers.len(texts) == 0;
            }
        }
    "#;

    let err = compile_one(source).unwrap_err();
    assert_has_code(&err, CftErrorCode::BitwiseRequiresIntOrFlagEnum);
    assert_has_code(&err, CftErrorCode::ShiftRequiresInt);
    assert_has_code(&err, CftErrorCode::UniqueUnsupportedElementType);
    assert_has_code(&err, CftErrorCode::FunctionArgTypeMismatch);
    assert_has_code(&err, CftErrorCode::FunctionArityMismatch);
}

#[test]
fn type_checker_accepts_dict_entry_keys_values_and_enum_constructor() {
    let source = r#"
        @flag
        enum Flags { A = 1, B = 2, }
        enum Damage { Fire, Ice, }

        type Item {
            flags: Flags;
            resistances: {Damage: float};
            names: {string: int};

            check {
                (flags & Flags.A) != Flags(0);
                resistances.contains(Damage.Fire);
                resistances.keys().len() >= 0;
                names.values().sum() >= 0;
                all entry in resistances {
                    entry.key >= Damage.Fire;
                    0.0 <= entry.value <= 1.0;
                }
            }
        }
    "#;

    compile_one(source).unwrap();
}

/// Regression: bare enum names (e.g. `Rarity > 5`, `Rarity + 1`,
/// `Rarity.len()`) used to surface as a generic `OperatorTypeMismatch` /
/// `FunctionArgTypeMismatch` without explaining that the *enum type itself*
/// was being used as a value. The diagnostic now mentions the enum and
/// suggests `EnumName.Variant` or `EnumName(0)`.
#[test]
fn type_checker_reports_bare_enum_name_used_as_value() {
    let source = r#"
        enum Rarity { Common, Rare, }
        type Item {
            rarity: Rarity;
            check {
                Rarity > 5;
                rarity == Rarity;
                Rarity.len() > 0;
            }
        }
    "#;
    let err = compile_one(source).unwrap_err();
    assert_has_code(&err, CftErrorCode::OperatorTypeMismatch);

    let mismatches: Vec<_> = err
        .diagnostics
        .iter()
        .filter(|d| d.code == CftErrorCode::OperatorTypeMismatch)
        .collect();
    assert!(
        mismatches
            .iter()
            .any(|d| d.message.contains("Rarity") && d.message.contains("Variant")),
        "expected diagnostic message to suggest `Rarity.Variant`, got {:?}",
        mismatches.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

#[test]
fn type_checker_reports_enum_constructor_and_dict_index_edges() {
    let source = r#"
        enum Damage { Fire, Ice, }
        type Item {
            resistances: {Damage: float};
            check {
                Damage("x") == Damage.Fire;
                Damage(0, 1) == Damage.Fire;
                resistances["Fire"] >= 0.0;
            }
        }
    "#;

    let err = compile_one(source).unwrap_err();
    assert_has_code(&err, CftErrorCode::FunctionArgTypeMismatch);
    assert_has_code(&err, CftErrorCode::FunctionArityMismatch);
    assert_has_code(&err, CftErrorCode::IndexTypeMismatch);
}

#[test]
fn type_checker_rejects_runtime_only_eval_edges_before_checker_runs() {
    let source = r#"
        enum Rarity { Common, Rare, }
        enum Element { Fire, Ice, }
        type Item {
            key: string;
            rarity: Rarity;
            nums: [int];
            texts: [string];
            resistances: {Element: float};
            check {
                key;
                when key { true; }
                MissingEnum(0) == Rarity.Common;
                Rarity("x") == rarity;
                rarity | Element.Fire == rarity;
                rarity << 1 == rarity;
                key.missing != "";
                key.len() > 0;
                nums.keys();
                nums.values();
                texts.sum() > 0;
                key.min() > 0;
                count.matches("[");
                key.missingFn();
                all entry in resistances {
                    entry.missing != 0;
                }
            }
        }
    "#;

    let err = compile_one(source).expect_err("invalid check expressions should be typed away");
    assert_has_code(&err, CftErrorCode::ConditionMustBeBool);
    assert_has_code(&err, CftErrorCode::UnknownFunction);
    assert_has_code(&err, CftErrorCode::FunctionArgTypeMismatch);
    assert_has_code(&err, CftErrorCode::BitwiseRequiresIntOrFlagEnum);
    assert_has_code(&err, CftErrorCode::ShiftRequiresInt);
    assert_has_code(&err, CftErrorCode::FieldAccessOnNonObject);
    assert_has_code(&err, CftErrorCode::UnknownField);
}

#[test]
fn type_checker_reports_builtin_arity_and_operator_edges() {
    let source = r#"
        @flag
        enum Flags { A = 1, B = 2, }
        enum Rarity { Common, Rare, }
        type Item {
            key: string;
            count: int;
            flags: Flags;
            rarity: Rarity;
            nums: [int];
            weights: {string: int};
            check {
                !key;
                -flags == flags;
                ~rarity == rarity;
                true + false == 0;
                1 // 1.0 == 1;
                key[0] != "";
                key.contains(1);
                nums.contains();
                key.isUnique(1);
                key.min(1);
                key.sum(1);
                key.keys(1);
                key.values(1);
                key.matches();
                key.matches("[");
                key.matches("[", "[");
                key.matches(key);
                all entry in weights {
                    entry.key > 0;
                }
            }
        }
    "#;

    let err = compile_one(source).expect_err("invalid expression edges should fail type checking");
    assert_has_code(&err, CftErrorCode::OperatorTypeMismatch);
    assert_has_code(&err, CftErrorCode::BitwiseRequiresIntOrFlagEnum);
    assert_has_code(&err, CftErrorCode::IndexOnNonIndexable);
    assert_has_code(&err, CftErrorCode::FunctionArgTypeMismatch);
    assert_has_code(&err, CftErrorCode::FunctionArityMismatch);
    assert_has_code(&err, CftErrorCode::InvalidRegexPattern);
    assert_has_code(&err, CftErrorCode::RegexPatternMustBeLiteral);
    assert_has_code(&err, CftErrorCode::ComparisonTypeMismatch);
}

#[test]
fn type_checker_reports_non_enum_variant_and_contains_dict_key_edges() {
    let source = r#"
        type Item {
            key: string;
            tags: {string: int};
            check {
                Item.Missing == 0;
                tags.contains(1);
            }
        }
    "#;

    let err = compile_one(source)
        .expect_err("non-enum variant access and wrong dict contains key should fail");
    assert_has_code(&err, CftErrorCode::TypeEnumVariantOnNonEnum);
    assert_has_code(&err, CftErrorCode::FunctionArgTypeMismatch);
}

#[test]
fn type_checker_allows_int_div_mod_shift_and_flag_bitnot_edges() {
    compile_one(
        r#"
            @flag
            enum Flags { A = 1, B = 2, }
            type Item {
                flags: Flags;
                check {
                    7 // 2 == 3;
                    7 % 2 == 1;
                    1 << 2 == 4;
                    4 >> 1 == 2;
                    ~flags != Flags(0);
                    !true;
                }
            }
        "#,
    )
    .expect("valid int-only and flag-only operators should type check");
}

#[test]
fn type_checker_suppresses_cascaded_operator_errors_when_operand_is_unknown() {
    let err = compile_one(
        r#"
            type Item {
                value: int;
                check {
                    missing && true;
                    missing + value > 0;
                    missing < value;
                    when missing { true; }
                }
            }
        "#,
    )
    .expect_err("unknown names should be reported without cascaded bool/operator errors");

    assert_has_code(&err, CftErrorCode::UnknownValueName);
    assert_eq!(
        err.diagnostics
            .iter()
            .filter(|diag| diag.code == CftErrorCode::ConditionMustBeBool)
            .count(),
        0,
        "unknown values should not cascade into condition diagnostics"
    );
    assert_eq!(
        err.diagnostics
            .iter()
            .filter(|diag| diag.code == CftErrorCode::OperatorTypeMismatch)
            .count(),
        0,
        "unknown values should not cascade into operator diagnostics"
    );
    assert_eq!(
        err.diagnostics
            .iter()
            .filter(|diag| diag.code == CftErrorCode::ComparisonTypeMismatch)
            .count(),
        0,
        "unknown values should not cascade into comparison diagnostics"
    );
}

#[test]
fn type_checker_rejects_unknown_unique_method_name() {
    let err = compile_one(
        r#"
            type Holder {
                nums: [int] = [];
                check { nums.unique(); }
            }
        "#,
    )
    .expect_err("unknown unique method name should be rejected");

    assert_has_code(&err, CftErrorCode::UnknownFunction);
}

#[test]
fn type_checker_suppresses_cascaded_function_and_field_errors_when_operand_is_unknown() {
    let err = compile_one(
        r#"
            type Item {
                nums: [int];
                attrs: {string: int};
                check {
                    nums[missing] > 0;
                    attrs[missing] > 0;
                    MissingEnum(missing) == 0;
                    nums.contains(missing);
                    attrs.contains(missing);
                    missing.matches("[");
                    missing.value == 0;
                    !missing;
                    missing | 1 == 1;
                }
            }
        "#,
    )
    .expect_err("unknown operands should not cascade into unrelated type errors");

    assert_has_code(&err, CftErrorCode::UnknownValueName);
    assert_eq!(
        err.diagnostics
            .iter()
            .filter(|diag| diag.code == CftErrorCode::IndexTypeMismatch)
            .count(),
        0,
        "unknown index operands should not emit index type mismatches"
    );
    assert_eq!(
        err.diagnostics
            .iter()
            .filter(|diag| diag.code == CftErrorCode::FunctionArgTypeMismatch)
            .count(),
        0,
        "unknown function operands should not emit function arg mismatches"
    );
    assert_eq!(
        err.diagnostics
            .iter()
            .filter(|diag| diag.code == CftErrorCode::FieldAccessOnNonObject)
            .count(),
        0,
        "unknown field bases should not emit field-access errors"
    );
    assert_eq!(
        err.diagnostics
            .iter()
            .filter(|diag| diag.code == CftErrorCode::OperatorTypeMismatch)
            .count(),
        0,
        "unknown unary/binary operands should not emit operator mismatches"
    );
}

#[test]
fn type_checker_reports_array_contains_dict_contains_and_matches_arg_edges() {
    let source = r#"
        enum Damage { Fire, Ice, }
        type Item {
            nums: [int];
            attrs: {Damage: int};
            key: string;
            count: int;
            check {
                nums.contains("1");
                attrs.contains("Fire");
                count.matches(".*");
            }
        }
    "#;

    let err = compile_one(source)
        .expect_err("wrong contains and matches argument types should fail type checking");
    assert_has_code(&err, CftErrorCode::FunctionArgTypeMismatch);
    assert_eq!(
        err.diagnostics
            .iter()
            .filter(|diag| diag.code == CftErrorCode::FunctionArgTypeMismatch)
            .count(),
        3
    );
}

#[test]
fn type_checker_accepts_scalar_formatted_values_and_rejects_collections() {
    compile_one(
        r#"
        enum Rarity { Common, Rare, }
        type Item {
            level: int;
            enabled: bool;
            rarity: Rarity;
            note: string?;
            check {
                id == f"{level}:{enabled}:{rarity}:{note}:{null}";
            }
        }
        "#,
    )
    .expect("scalar, enum, nullable scalar, and null interpolation should compile");

    let err = compile_one(
        r#"
        type Item {
            nums: [int];
            attrs: {string: int};
            check {
                id == f"{nums}";
                id == f"{attrs}";
            }
        }
        "#,
    )
    .expect_err("collection interpolation must fail type checking");
    assert_eq!(
        err.diagnostics
            .iter()
            .filter(|diag| diag.code == CftErrorCode::OperatorTypeMismatch)
            .count(),
        2
    );
}

#[test]
fn type_checker_rejects_invalid_extended_builtin_types() {
    let err = compile_one(
        r#"
        type Item {
            nullable_nums: [int?];
            floats: [float];
            text: string;
            count: int;
            check {
                nullable_nums.isSorted();
                floats.intersects(floats);
                text.abs() == 0;
                count.startsWith("1");
            }
        }
        "#,
    )
    .expect_err("unsupported extended builtin types must fail");
    assert!(
        err.diagnostics
            .iter()
            .filter(|diag| diag.code == CftErrorCode::FunctionArgTypeMismatch)
            .count()
            >= 4
    );
}
