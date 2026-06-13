//! Coverage for the migration-driven annotation extensions:
//!   - `@id` / `@ref` / `@index` accept enum-typed fields
//!   - `@expand` parent fields must reference a concrete type
//!   - `@IdAsEnum("Name")` declares generated enums on string @id fields
//!   - `@GenAsEnum("Name")` references generated enums on string fields

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
fn key_as_enum_compiles_on_string_id_field() {
    compile_one(
        r#"
            type Item {
                @IdAsEnum("ItemId")
                @id
                id: string;
                name: string;
            }
        "#,
    )
    .expect("@IdAsEnum should compile on a string @id field");
}

#[test]
fn gen_as_enum_compiles_on_string_field_when_declared_by_id_field() {
    compile_one(
        r#"
            type Item {
                @id
                @IdAsEnum("ItemName")
                id: string;
            }
            type Modifier {
                @id
                id: string;
                @GenAsEnum("ItemName")
                name: string;
            }
        "#,
    )
    .expect("@GenAsEnum should compile on a string field when an @id field declares it");
}

#[test]
fn gen_as_enum_on_string_field_requires_declared_enum() {
    let err = compile_one(
        r#"
            type Modifier {
                @id
                id: string;
                @GenAsEnum("MissingName")
                name: string;
            }
        "#,
    )
    .expect_err("@GenAsEnum on a string field should reference a declared enum");
    assert_has_code(&err, CftErrorCode::InvalidAnnotationArgument);
}

#[test]
fn id_as_enum_on_non_id_field_is_rejected() {
    let err = compile_one(
        r#"
            type Item {
                @id
                @IdAsEnum("ItemId")
                id: string;
                @IdAsEnum("ItemName")
                name: string;
            }
        "#,
    )
    .expect_err("@IdAsEnum requires @id");
    assert_has_code(&err, CftErrorCode::InvalidAnnotatedFieldType);
}

#[test]
fn key_as_enum_is_not_supported() {
    let err = compile_one(
        r#"
            type Item {
                @id
                @KeyAsEnum("ItemId")
                id: string;
            }
        "#,
    )
    .expect_err("@KeyAsEnum should not be supported");
    assert_has_code(&err, CftErrorCode::UnknownAnnotation);
}

#[test]
fn key_as_enum_on_non_string_id_field_is_rejected() {
    let err = compile_one(
        r#"
            type Item {
                @IdAsEnum("ItemId")
                @id
                id: int;
            }
        "#,
    )
    .expect_err("@IdAsEnum requires string field type");
    assert_has_code(&err, CftErrorCode::InvalidAnnotatedFieldType);
}

#[test]
fn key_as_enum_rejects_invalid_csharp_enum_name() {
    let err = compile_one(
        r#"
            type Item {
                @IdAsEnum("1Bad")
                @id
                id: string;
            }
        "#,
    )
    .expect_err("@IdAsEnum enum name must be a C# identifier");
    assert_has_code(&err, CftErrorCode::InvalidAnnotationArgument);
}

#[test]
fn key_as_enum_rejects_existing_global_name() {
    let err = compile_one(
        r#"
            enum ItemId { Existing }
            type Item {
                @IdAsEnum("ItemId")
                @id
                id: string;
            }
        "#,
    )
    .expect_err("@IdAsEnum enum name must not collide with schema globals");
    assert_has_code(&err, CftErrorCode::DuplicateGlobalName);
}

#[test]
fn key_as_enum_rejects_duplicate_generated_enum_name() {
    let err = compile_one(
        r#"
            type Item {
                @IdAsEnum("SharedId")
                @id
                id: string;
            }
            type Quest {
                @IdAsEnum("SharedId")
                @id
                id: string;
            }
        "#,
    )
    .expect_err("@IdAsEnum enum names must be unique");
    assert_has_code(&err, CftErrorCode::DuplicateGlobalName);
}
