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
fn key_as_enum_compiles_on_type() {
    let schema = compile_one(
        r#"
            @keyAsEnum("SkillKey")
            type Skill {
                name: string;
            }
        "#,
    )
    .expect("@keyAsEnum should compile on a type");

    let skill = schema.resolve_type("Skill").expect("Skill type");
    assert_eq!(skill.annotations[0].name, "keyAsEnum");
    assert_eq!(
        skill.annotations[0].args,
        vec![coflow_cft::CftAnnotationValue::String(
            "SkillKey".to_string()
        )]
    );
}

#[test]
fn key_as_enum_requires_single_string_argument_and_type_target() {
    let invalid_arg = compile_one(
        r#"
            @keyAsEnum(SkillKey)
            type Skill {
                name: string;
            }
        "#,
    )
    .expect_err("@keyAsEnum requires a string argument");
    assert_has_code(&invalid_arg, CftErrorCode::InvalidAnnotationArgument);

    let invalid_target = compile_one(
        r#"
            type Skill {
                @keyAsEnum("SkillKey")
                name: string;
            }
        "#,
    )
    .expect_err("@keyAsEnum is type-only");
    assert_has_code(&invalid_target, CftErrorCode::InvalidAnnotationTarget);
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
        "type Target { key: string; } type Holder { @ref(Target) target: string; }",
        "type Skill { @index name: string; }",
        "type Skill { @IdAsEnum(\"SkillKey\") key: string; }",
        "type Skill { @GenAsEnum(\"SkillKey\") key: string; }",
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
