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
fn schema_reports_cross_module_duplicate_with_related_label() {
    let mut container = CftContainer::new();
    container
        .add_module(ModuleId::from("a"), "type Item { key: string; }")
        .unwrap();
    container
        .add_module(ModuleId::from("b"), "enum Item { A, }")
        .unwrap();
    let err = container.compile().unwrap_err();
    assert_has_code(&err, CftErrorCode::DuplicateGlobalName);
    let diag = err
        .diagnostics
        .iter()
        .find(|diag| diag.code == CftErrorCode::DuplicateGlobalName)
        .unwrap();
    assert!(!diag.related.is_empty());
}

#[test]
fn schema_accepts_builtin_method_calls_and_rejects_global_builtins() {
    compile_one(
        r#"
        type Item {
            key: string;
            tags: [string];
            attrs: {string: int};
            check {
                key.matches("^[a-z]+$");
                tags.len() > 0;
                tags.contains("weapon");
                tags.unique();
                attrs.keys().contains("power");
                attrs.values().sum() >= 1;
            }
        }
        "#,
    )
    .unwrap();

    let err = compile_one(
        r#"
        type Item {
            tags: [string];
            check { len(tags) > 0; }
        }
        "#,
    )
    .unwrap_err();
    assert_has_code(&err, CftErrorCode::UnknownFunction);
}

#[test]
fn schema_reports_duplicate_field_enum_value_and_unknown_type() {
    let source = r#"
        enum E { A = 1, B = 1, }
        type A { x: Missing; x: int; }
    "#;
    let err = compile_one(source).unwrap_err();
    assert_has_code(&err, CftErrorCode::DuplicateEnumValue);
    assert_has_code(&err, CftErrorCode::DuplicateFieldName);
    assert_has_code(&err, CftErrorCode::UnknownNamedType);
}

#[test]
fn schema_rejects_reserved_identifiers() {
    let cases = [
        "const true = 1;",
        "type type { value: string; }",
        "type int { value: string; }",
        "enum len { A, }",
        "enum E { true, }",
        "const match = 1;",
        "const export = 1;",
        "type Item { type: string; }",
        "type Item { true: bool; }",
        "type Item { check: int; }",
        "type Item { from: string; }",
        "type Item { id: string; }",
        "enum E { _, }",
        "type Item { values: [int]; check { all all in values { true; } } }",
    ];

    for source in cases {
        let err = compile_one(source).expect_err(source);
        assert_has_code(&err, CftErrorCode::ReservedIdentifier);
    }
}

#[test]
fn schema_allows_underscore_prefixed_identifiers() {
    compile_one("type _Internal { _value: int; }").expect("underscore-prefixed names are valid");
}

#[test]
fn schema_reports_inheritance_and_modifier_errors() {
    let source = r#"
        sealed type Parent { key: string; }
        abstract sealed type Bad { x: int; }
        type Child : Parent { key: string; }
        type A : B { x: int; }
        type B : A { y: int; }
    "#;
    let err = compile_one(source).unwrap_err();
    assert_has_code(&err, CftErrorCode::InheritSealedType);
    assert_has_code(&err, CftErrorCode::DuplicateInheritedField);
    assert_has_code(&err, CftErrorCode::ConflictingTypeModifiers);
    assert_has_code(&err, CftErrorCode::InheritanceCycle);
}

#[test]
fn schema_reports_removed_record_annotations_and_flag_errors() {
    let source = r#"
        @flag
        enum Flags { A = 1, B = 3, }

        @struct
        type NotSealed { x: int; }

        type OldAnnotations {
            @id
            key: string;
            @ref(Flags)
            flag: string;
            @index
            name: string;
        }
    "#;
    let err = compile_one(source).unwrap_err();
    assert_has_code(&err, CftErrorCode::InvalidFlagEnumValue);
    assert_has_code(&err, CftErrorCode::StructRequiresSealedType);
    assert_has_code(&err, CftErrorCode::UnknownAnnotation);
}

#[test]
fn schema_reports_default_errors() {
    let source = r#"
        const NAME = "x";
        enum Rarity { Common, }
        type Item {
            key: int = NAME;
            bad: int = Missing;
            field_ref: int = key;
            rarity: Rarity = Rarity.Missing;
            xs: [int] = [1];
        }
    "#;
    let err = compile_one(source).unwrap_err();
    assert_has_code(&err, CftErrorCode::DefaultTypeMismatch);
    assert_has_code(&err, CftErrorCode::UnknownConst);
    assert_has_code(&err, CftErrorCode::DefaultReferencesField);
    assert_has_code(&err, CftErrorCode::UnknownEnumVariant);
    assert_has_code(&err, CftErrorCode::InvalidDefaultExpression);
}

#[test]
fn schema_accepts_empty_array_and_object_defaults_only_for_matching_composites() {
    let schema = compile_one(
        r#"
            type Stats { hp: int = 10; }
            type Item {
                tags: [string] = [];
                attrs: {string: int} = {};
                stats: Stats = {};
            }
        "#,
    )
    .expect("empty composite defaults should compile for matching fields");

    let item = schema.resolve_type("Item").expect("Item type");
    assert_eq!(
        item.fields[0].default,
        Some(coflow_cft::CftSchemaDefaultValue::EmptyArray)
    );
    assert_eq!(
        item.fields[1].default,
        Some(coflow_cft::CftSchemaDefaultValue::EmptyObject)
    );
    assert_eq!(
        item.fields[2].default,
        Some(coflow_cft::CftSchemaDefaultValue::EmptyObject)
    );

    let err = compile_one(
        r#"
            type Bad {
                number: int = {};
                words: [string] = {};
            }
        "#,
    )
    .unwrap_err();
    assert_has_code(&err, CftErrorCode::DefaultTypeMismatch);
}

#[test]
fn schema_reports_enum_default_on_non_enum_and_unknown_enum_names() {
    let err = compile_one(
        r#"
            const Prefix = "item";
            type Item {
                from_const: string = Prefix.Common;
                from_missing: string = Missing.Common;
            }
        "#,
    )
    .unwrap_err();

    let diagnostics = err
        .diagnostics
        .iter()
        .filter(|diag| diag.code == CftErrorCode::EnumVariantOnNonEnum)
        .collect::<Vec<_>>();
    assert_eq!(diagnostics.len(), 2);
    assert!(
        diagnostics.iter().any(|diag| !diag.related.is_empty()),
        "non-enum symbol should include related definition"
    );
}

#[test]
fn schema_reports_parent_field_default_references() {
    let source = r#"
        type Base { base_key: int; }
        type Child : Base {
            copy: int = base_key;
        }
    "#;

    let err = compile_one(source).unwrap_err();
    assert_has_code(&err, CftErrorCode::DefaultReferencesField);
}

#[test]
fn schema_accepts_explicit_i64_max_enum_value_without_following_auto_variant() {
    let mut container = compile_one("enum Limit { Max = 9223372036854775807, }").unwrap();
    container.compile().unwrap();

    let enum_schema = container.resolve_enum("Limit").unwrap();
    assert_eq!(enum_schema.variants[0].value, i64::MAX);
}

#[test]
fn schema_accepts_zero_flag_value_and_rejects_negative_flag_values() {
    compile_one(
        r#"
            @flag
            enum Permissions {
                None = 0,
                Read = 1,
                Write = 2,
            }
        "#,
    )
    .expect("zero and powers of two are valid @flag enum values");

    let err = compile_one(
        r#"
            @flag
            enum Permissions {
                Invalid = -1,
            }
        "#,
    )
    .unwrap_err();
    assert_has_code(&err, CftErrorCode::InvalidFlagEnumValue);
}

#[test]
fn schema_accepts_display_and_deprecated_on_enum_variants() {
    let schema = compile_one(
        r#"
            enum Rarity {
                @display("Common display")
                Common,
                @deprecated
                Old,
            }
        "#,
    )
    .expect("variant annotations should compile");

    let rarity = schema.resolve_enum("Rarity").expect("enum");
    assert_eq!(rarity.variants[0].annotations[0].name, "display");
    assert_eq!(rarity.variants[1].annotations[0].name, "deprecated");
}

#[test]
fn schema_rejects_invalid_enum_variant_annotations() {
    let err = compile_one(
        r#"
            enum Rarity {
                @keyAsEnum(RarityKey)
                Common,
            }
        "#,
    )
    .expect_err("invalid variant annotation should fail");
    assert_has_code(&err, CftErrorCode::InvalidAnnotationTarget);
}

#[test]
fn schema_rejects_duplicate_annotations_and_invalid_annotation_arguments() {
    let source = r#"
        @flag(1)
        enum Flags { A = 1, }

        @keyAsEnum(ItemKey)
        @keyAsEnum(ItemKey2)
        type Holder {
            key: string;

            @display
            name: string;
        }
    "#;
    let err = compile_one(source).unwrap_err();
    assert_has_code(&err, CftErrorCode::DuplicateAnnotation);
    assert_has_code(&err, CftErrorCode::InvalidAnnotationArgument);
}

/// Regression for B4: the lexer used to parse the magnitude as `i64`, so
/// `-9223372036854775808` (i.e. `i64::MIN`) was rejected with InvalidIntLiteral
/// because `9223372036854775808` doesn't fit in i64. Magnitudes that exceed
/// `i64::MAX` are now lexed as a special token and only legal under unary
/// minus, allowing `i64::MIN` exactly.
#[test]
fn schema_accepts_i64_min_in_enum_const_and_default() {
    let source = r#"
        const MIN_LEVEL: int = -9223372036854775808;
        enum Edge { Bottom = -9223372036854775808, }
        type Edged { value: int = -9223372036854775808; }
    "#;
    let container = compile_one(source).unwrap();
    assert!(matches!(
        container.resolve_const("MIN_LEVEL").unwrap().value,
        CftConstValue::Int(i64::MIN),
    ));
    let edge = container.resolve_enum("Edge").unwrap();
    assert_eq!(edge.variants[0].value, i64::MIN);
}

#[test]
fn schema_rejects_unsigned_magnitude_that_exceeds_i64_min() {
    // 2^63 + 1 is out of range even with a unary minus.
    let err = compile_one("const C: int = -9223372036854775809;").unwrap_err();
    assert_has_code(&err, CftErrorCode::InvalidIntLiteral);
}

#[test]
fn schema_rejects_bare_unsigned_magnitude_above_i64_max() {
    let err = compile_one("const C: int = 9223372036854775808;").unwrap_err();
    assert_has_code(&err, CftErrorCode::InvalidIntLiteral);
}

#[test]
fn schema_reports_enum_auto_numbering_overflow_only_when_next_variant_needs_value() {
    let err = compile_one("enum Limit { Max = 9223372036854775807, Next, }").unwrap_err();
    assert_has_code(&err, CftErrorCode::InvalidEnumValueSequence);
}

/// Regression: an unknown field type used to be reported once per stage
/// (`validate_field_shapes`, `validate_field_annotations`,
/// `validate_defaults`, `build_full_fields`). The first pass now owns
/// emission, so a single unknown name yields exactly one diagnostic even
/// when the field carries multiple annotations and a default.
#[test]
fn schema_does_not_duplicate_unknown_field_type_diagnostic() {
    let cases = [
        "type T { x: Missing; }",
        "type T { @display(\"x\") x: Missing; }",
        "const C = 1; type T { x: Missing = C; }",
    ];
    for source in cases {
        let err = compile_one(source).unwrap_err();
        let count = err
            .diagnostics
            .iter()
            .filter(|d| d.code == CftErrorCode::UnknownNamedType)
            .count();
        assert_eq!(
            count, 1,
            "expected exactly one UnknownNamedType for `{source}`, got {count}"
        );
    }
}

/// Regression: `{ [int]: int }` used to push InvalidDictKeyType twice — once
/// during `validate_field_shapes` and again during `build_full_fields`.
#[test]
fn schema_does_not_duplicate_invalid_dict_key_diagnostic() {
    let err = compile_one("type T { d: {[int]: int}; }").unwrap_err();
    let count = err
        .diagnostics
        .iter()
        .filter(|d| d.code == CftErrorCode::InvalidDictKeyType)
        .count();
    assert_eq!(
        count, 1,
        "expected exactly one InvalidDictKeyType, got {count}"
    );
}

/// Regression: `@struct` on an `enum` used to emit two
/// `InvalidAnnotationTarget` diagnostics — once from the generic target
/// validator and once from a duplicated `@struct`-only branch.
#[test]
fn schema_emits_single_invalid_annotation_target_for_struct_on_enum() {
    let err = compile_one("@struct enum E { A, }").unwrap_err();
    let count = err
        .diagnostics
        .iter()
        .filter(|d| d.code == CftErrorCode::InvalidAnnotationTarget)
        .count();
    assert_eq!(
        count, 1,
        "expected one InvalidAnnotationTarget for @struct on enum, got {count}"
    );
}
