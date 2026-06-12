//! Coverage for the migration-driven annotation extensions:
//!   - `@id` / `@ref` / `@index` accept enum-typed fields
//!   - `@expand` parent fields must reference a concrete type
//!   - `@KeyAsEnumValue` is accepted on type definitions

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
fn enum_typed_id_compiles_without_annotation_error() {
    compile_one(
        r#"
            enum Color { Red = 0, Green = 1, }
            type Palette {
                @id
                id: Color;
                name: string;
            }
        "#,
    )
    .expect("enum-typed @id should compile");
}

#[test]
fn enum_typed_ref_compiles_without_annotation_error() {
    compile_one(
        r#"
            enum Color { Red = 0, Green = 1, }
            type Palette {
                @id
                id: Color;
                name: string;
            }
            type Brush {
                @id
                bid: string;
                @ref(Palette)
                color: Color;
            }
        "#,
    )
    .expect("enum-typed @ref should compile");
}

#[test]
fn enum_typed_ref_id_mismatch_still_caught() {
    let err = compile_one(
        r#"
            enum Color { Red = 0, Green = 1, }
            enum Mood { Happy = 0, Sad = 1, }
            type Palette {
                @id
                id: Color;
                name: string;
            }
            type Brush {
                @id
                bid: string;
                @ref(Palette)
                color: Mood;
            }
        "#,
    )
    .expect_err("ref id mismatch should still error even with enums");
    assert_has_code(&err, CftErrorCode::RefIdTypeMismatch);
}

#[test]
fn expand_on_concrete_type_field_compiles() {
    compile_one(
        r#"
            @struct sealed type Position { x: float; y: float; }
            type Anchor {
                @id
                id: string;
                @expand
                pos: Position;
            }
        "#,
    )
    .expect("@expand on a concrete-type field should compile");
}

#[test]
fn expand_on_primitive_field_is_rejected() {
    let err = compile_one(
        r#"
            type Anchor {
                @id
                id: string;
                @expand
                value: int;
            }
        "#,
    )
    .expect_err("@expand requires a concrete type");
    assert_has_code(&err, CftErrorCode::InvalidAnnotatedFieldType);
}

#[test]
fn expand_on_enum_field_is_rejected() {
    let err = compile_one(
        r#"
            enum Color { Red = 0, Green = 1, }
            type Anchor {
                @id
                id: string;
                @expand
                color: Color;
            }
        "#,
    )
    .expect_err("@expand on an enum field should be rejected");
    assert_has_code(&err, CftErrorCode::InvalidAnnotatedFieldType);
}

#[test]
fn expand_on_array_field_is_rejected() {
    let err = compile_one(
        r#"
            @struct sealed type Position { x: float; y: float; }
            type Anchor {
                @id
                id: string;
                @expand
                positions: [Position];
            }
        "#,
    )
    .expect_err("@expand on a list field should be rejected");
    assert_has_code(&err, CftErrorCode::InvalidAnnotatedFieldType);
}

#[test]
fn expand_on_nullable_field_is_rejected() {
    let err = compile_one(
        r#"
            @struct sealed type Position { x: float; y: float; }
            type Anchor {
                @id
                id: string;
                @expand
                pos: Position?;
            }
        "#,
    )
    .expect_err("@expand on a nullable field should be rejected");
    assert_has_code(&err, CftErrorCode::InvalidAnnotatedFieldType);
}

#[test]
fn key_as_enum_value_compiles_on_string_id_table() {
    compile_one(
        r#"
            @KeyAsEnumValue
            type Item {
                @id
                id: string;
                name: string;
            }
        "#,
    )
    .expect("@KeyAsEnumValue should compile on a type with string @id");
}

#[test]
fn key_as_enum_value_on_field_is_rejected() {
    let err = compile_one(
        r#"
            type Item {
                @KeyAsEnumValue
                @id
                id: string;
                name: string;
            }
        "#,
    )
    .expect_err("@KeyAsEnumValue is type-level only");
    assert_has_code(&err, CftErrorCode::InvalidAnnotationTarget);
}
