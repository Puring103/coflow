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
    assert_eq!(skill.id_as_enum.as_deref(), Some("SkillKey"));
    assert_eq!(
        schema
            .type_for_id_as_enum("SkillKey")
            .map(|ty| ty.name.as_str()),
        Some("Skill")
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
        "type Target { value: string; } type Holder { @ref target: Target; }",
        "type Target { value: string; } type Holder { @inline target: Target; }",
        "type Target { value: string; } type Holder { @ref(Target) target: Target; }",
    ] {
        let err = compile_one(source).expect_err(source);
        assert_has_code(&err, CftErrorCode::UnknownAnnotation);
    }
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
    for source in [
        r#"
            type Anchor {
                @expand
                value: int;
            }
        "#,
        r#"
            type Position { x: float; y: float; }
            type Anchor {
                @expand
                pos: Position?;
            }
        "#,
    ] {
        let err = compile_one(source).expect_err("@expand requires a concrete type");
        assert_has_code(&err, CftErrorCode::InvalidAnnotatedFieldType);
    }
}

#[test]
fn expand_on_abstract_or_singleton_object_field_is_rejected() {
    for source in [
        r#"
            abstract type Base { value: int; }
            type Holder {
                @expand
                base: Base;
            }
        "#,
        r#"
            @singleton
            type Settings { value: int; }
            type Holder {
                @expand
                settings: Settings;
            }
        "#,
    ] {
        let err = compile_one(source).expect_err(source);
        assert_has_code(&err, CftErrorCode::InvalidAnnotatedFieldType);
    }
}
