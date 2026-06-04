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
