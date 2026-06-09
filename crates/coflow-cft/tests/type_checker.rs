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
            id: string;
            rarity: Rarity;
            tags: [string];
            scores: {string: int};
            check {
                missing != "";
                id.missing != "";
                Rarity.Missing == rarity;
                rarity > 5;
                len(id);
                all ch in id { ch != ""; }
                tags["x"] != "";
                matches(id, PAT);
                matches(id, "[");
            }
        }
    "#;
    let err = compile_one(source).unwrap_err();
    assert_has_code(&err, CftErrorCode::UnknownValueName);
    assert_has_code(&err, CftErrorCode::FieldAccessOnNonObject);
    assert_has_code(&err, CftErrorCode::TypeUnknownEnumVariant);
    assert_has_code(&err, CftErrorCode::ComparisonTypeMismatch);
    assert_has_code(&err, CftErrorCode::FunctionArgTypeMismatch);
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
fn type_checker_accepts_nullable_guarded_access_and_ref_object_view() {
    let source = r#"
        type Item {
            @id
            id: string;
            rarity: int;
        }

        type Holder {
            maybe: Item? = null;

            @ref(Item)
            item_id: string;

            check {
                maybe != null && maybe.id != "";
                item_id.id != "";
                item_id.rarity >= 0;
            }
        }
    "#;

    compile_one(source).unwrap();
}

#[test]
fn type_checker_reports_is_predicate_and_condition_edges() {
    let source = r#"
        enum E { A, }
        type Item {
            id: string;
            check {
                id is E;
                id;
                when id { true; }
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
            id: string;
            check {
                id is Item;
            }
        }
    "#;

    let err = compile_one(source).unwrap_err();
    assert_has_code(&err, CftErrorCode::OperatorTypeMismatch);
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
                unique(floats);
                unique(objects);
                min(texts) != "";
                sum(texts) == 0;
                contains(numbers, "x");
                len(numbers, texts) == 0;
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
                contains(resistances, Damage.Fire);
                len(keys(resistances)) >= 0;
                sum(values(names)) >= 0;
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
/// `len(Rarity)`) used to surface as a generic `OperatorTypeMismatch` /
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
                len(Rarity) > 0;
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
