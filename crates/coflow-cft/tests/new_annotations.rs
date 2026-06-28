//! Coverage for the record-key annotation model.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::panic_in_result_fn,
    clippy::needless_raw_string_hashes
)]

mod common;
use common::*;

#[test]
fn id_as_enum_compiles_on_type() {
    let schema = compile_one(
        r#"
            @idAsEnum(SkillKey)
            type Skill {
                name: string;
            }

            enum SkillKey {}

            type SkillUse {
                skill: SkillKey;
            }
        "#,
    )
    .expect("@idAsEnum should compile on a type");

    let skill = schema.resolve_type("Skill").expect("Skill type");
    assert_eq!(skill.annotations[0].name, "idAsEnum");
    assert_eq!(
        skill.annotations[0].args,
        vec![coflow_cft::CftAnnotationValue::Name("SkillKey".to_string())]
    );
    assert!(schema.resolve_enum("SkillKey").is_some());
}

#[test]
fn id_as_enum_requires_single_enum_name_argument_and_type_target() {
    let invalid_arg = compile_one(
        r#"
            @idAsEnum("SkillKey")
            type Skill {
                name: string;
            }
            enum SkillKey {}
        "#,
    )
    .expect_err("@idAsEnum requires an enum name argument");
    assert_has_code(&invalid_arg, CftErrorCode::InvalidAnnotationArgument);

    let invalid_target = compile_one(
        r#"
            type Skill {
                @idAsEnum(SkillKey)
                name: string;
            }
            enum SkillKey {}
        "#,
    )
    .expect_err("@idAsEnum is type-only");
    assert_has_code(&invalid_target, CftErrorCode::InvalidAnnotationTarget);
}

#[test]
fn id_as_enum_requires_existing_empty_enum_placeholder() {
    let missing = compile_one(
        r#"
            @idAsEnum(SkillKey)
            type Skill {
                name: string;
            }
        "#,
    )
    .expect_err("@idAsEnum requires an existing enum");
    assert_has_code(&missing, CftErrorCode::UnknownNamedType);

    let non_enum = compile_one(
        r#"
            @idAsEnum(SkillKey)
            type Skill {
                name: string;
            }
            type SkillKey {}
        "#,
    )
    .expect_err("@idAsEnum requires an enum, not a type");
    assert_has_code(&non_enum, CftErrorCode::IdAsEnumRequiresEmptyEnum);

    let with_variants = compile_one(
        r#"
            @idAsEnum(SkillKey)
            type Skill {
                name: string;
            }
            enum SkillKey {
                Fireball,
            }
        "#,
    )
    .expect_err("@idAsEnum placeholder enum cannot declare variants");
    assert_has_code(&with_variants, CftErrorCode::IdAsEnumRequiresEmptyEnum);
}

#[test]
fn id_is_reserved_as_a_field_name() {
    let err = compile_one(
        r#"
            type Skill {
                id: string;
                name: string;
            }
        "#,
    )
    .expect_err("id is reserved for record keys");

    assert_has_code(&err, CftErrorCode::ReservedIdentifier);
}

#[test]
fn old_record_annotations_are_rejected() {
    for source in [
        "type Skill { @id key: string; }",
        "type Skill { @index name: string; }",
        "type Skill { @IdAsEnum(\"SkillKey\") key: string; }",
        "type Skill { @GenAsEnum(\"SkillKey\") key: string; }",
    ] {
        let err = compile_one(source).expect_err(source);
        assert_has_code(&err, CftErrorCode::UnknownAnnotation);
    }

    let old_ref =
        compile_one("type Target { value: string; } type Holder { @ref(Target) target: Target; }")
            .expect_err("old @ref(Target) syntax should be rejected");
    assert_has_code(&old_ref, CftErrorCode::InvalidAnnotationArgument);
}

#[test]
fn expand_on_concrete_type_field_still_compiles() {
    compile_one(
        r#"
            @struct sealed type Position { x: float; y: float; }
            type Anchor {
                @expand
                pos: Position;
            }
        "#,
    )
    .expect("@expand on a concrete-type field should compile");
}

#[test]
fn expand_on_non_concrete_field_is_rejected() {
    let err = compile_one(
        r#"
            type Anchor {
                @expand
                value: int;
            }
        "#,
    )
    .expect_err("@expand requires a concrete type");

    assert_has_code(&err, CftErrorCode::InvalidAnnotatedFieldType);
}

#[test]
fn ref_and_inline_annotations_compile_on_object_shapes() {
    let schema = compile_one(
        r#"
            type Item { name: string; }
            type Holder {
                @ref
                item: Item;
                @inline
                maybe_item: Item?;
                @ref
                rewards: [Item];
                @inline
                by_name: {string: Item};
                @inline
                @expand
                expanded: Item;
            }
        "#,
    )
    .expect("@ref/@inline should compile on object-bearing fields");

    let holder = schema.resolve_type("Holder").expect("Holder type");
    assert!(
        holder.fields[0]
            .annotations
            .iter()
            .any(|annotation| annotation.name == "ref"),
        "item should carry @ref"
    );
    assert!(
        holder.fields[1]
            .annotations
            .iter()
            .any(|annotation| annotation.name == "inline"),
        "maybe_item should carry @inline"
    );
}

#[test]
fn ref_and_inline_annotations_reject_invalid_targets_and_conflicts() {
    let non_object = compile_one(
        r#"
            type Bad {
                @ref
                value: int;
            }
        "#,
    )
    .expect_err("@ref requires an object-bearing field");
    assert_has_code(&non_object, CftErrorCode::InvalidAnnotatedFieldType);

    let conflict = compile_one(
        r#"
            type Item { name: string; }
            type Bad {
                @ref
                @inline
                item: Item;
            }
        "#,
    )
    .expect_err("@ref and @inline are mutually exclusive");
    assert_has_code(&conflict, CftErrorCode::InvalidAnnotatedFieldType);

    let ref_expand = compile_one(
        r#"
            type Item { name: string; }
            type Bad {
                @ref
                @expand
                item: Item;
            }
        "#,
    )
    .expect_err("@ref conflicts with @expand");
    assert_has_code(&ref_expand, CftErrorCode::InvalidAnnotatedFieldType);

    let bad_args = compile_one(
        r#"
            type Item { name: string; }
            type Bad {
                @inline("yes")
                item: Item;
            }
        "#,
    )
    .expect_err("@inline has no arguments");
    assert_has_code(&bad_args, CftErrorCode::InvalidAnnotationArgument);
}
