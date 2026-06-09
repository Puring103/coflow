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
        .add_module(ModuleId::from("a"), "type Item { id: string; }")
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
        "type int { value: string; }",
        "enum len { A, }",
        "const match = 1;",
        "type Item { from: string; }",
        "enum E { _, }",
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
        sealed type Parent { id: string; }
        abstract sealed type Bad { x: int; }
        type Child : Parent { id: string; }
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
fn schema_reports_id_annotation_and_flag_errors() {
    let source = r#"
        @flag
        enum Flags { A = 1, B = 3, }

        type Base { @id id: string; }
        type Child : Base { @id other: int; }

        @struct
        type NotSealed { x: int; }

        type BadRef {
            @ref(Flags)
            flag_id: string;
            @index
            xs: [int];
        }
    "#;
    let err = compile_one(source).unwrap_err();
    assert_has_code(&err, CftErrorCode::InvalidFlagEnumValue);
    assert_has_code(&err, CftErrorCode::MultipleIdFieldsInTree);
    assert_has_code(&err, CftErrorCode::StructRequiresSealedType);
    assert_has_code(&err, CftErrorCode::RefTargetMustBeType);
    assert_has_code(&err, CftErrorCode::InvalidAnnotatedFieldType);
}

#[test]
fn schema_rejects_nullable_index_fields() {
    for source in [
        "type A { @index value: string? = null; }",
        "type A { @index value: int? = null; }",
        "enum E { A, } type A { @index value: E? = null; }",
    ] {
        let err = compile_one(source).unwrap_err();
        assert_has_code(&err, CftErrorCode::InvalidAnnotatedFieldType);
    }
}

#[test]
fn schema_reports_default_errors() {
    let source = r#"
        const NAME = "x";
        enum Rarity { Common, }
        type Item {
            id: int = NAME;
            bad: int = Missing;
            field_ref: int = id;
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
fn schema_reports_parent_field_default_references() {
    let source = r#"
        type Base { base_id: int; }
        type Child : Base {
            copy: int = base_id;
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
        "type T { @id x: Missing; }",
        "type T { @id @display(\"x\") x: Missing; }",
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

/// Regression: `MultipleIdFieldsInTree` used to report the alphabetically
/// first type as the "original" and the rest as duplicates, even when source
/// order said otherwise. The traversal now walks the inheritance tree from
/// the root downwards, so the parent's `@id` is always recorded as the
/// canonical declaration regardless of how the types are named.
#[test]
fn schema_reports_id_conflict_with_parent_first_regardless_of_alphabetical_order() {
    // Parent name "Z" sorts after child name "A" alphabetically.
    let source = "type Z { @id z_id: string; } type A : Z { @id a_id: string; }";
    let err = compile_one(source).unwrap_err();
    let diag = err
        .diagnostics
        .iter()
        .find(|d| d.code == CftErrorCode::MultipleIdFieldsInTree)
        .expect("MultipleIdFieldsInTree diagnostic");
    let primary = diag.primary.as_ref().expect("primary label");
    let related = diag.related.first().expect("related label");

    let parent_id_offset = source.find("z_id").expect("z_id span");
    let child_id_offset = source.find("a_id").expect("a_id span");

    assert_eq!(
        primary.span.start, child_id_offset,
        "primary should point at the redundant child @id"
    );
    assert_eq!(
        related.span.start, parent_id_offset,
        "related should point at the original parent @id"
    );
}
