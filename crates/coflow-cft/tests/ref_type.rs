#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::panic_in_result_fn,
    clippy::needless_raw_string_hashes
)]

mod common;
use coflow_cft::CftSchemaTypeRef;
use common::*;

#[test]
fn ref_type_compiles_object_references_and_nested_shapes() {
    let schema = compile_one(
        r#"
            type Item { name: string; }
            type Holder {
                item: &Item;
                backup: &Item? = null;
                items: [&Item];
                by_name: {string: &Item};
            }
        "#,
    )
    .expect("& object reference fields should compile");

    let holder = schema.resolve_type("Holder").expect("Holder type");
    assert_eq!(holder.fields[0].ty, "&Item");
    assert_eq!(
        holder.fields[0].ty_ref,
        CftSchemaTypeRef::Ref("Item".to_string())
    );
    assert_eq!(holder.fields[1].ty, "&Item?");
    assert_eq!(
        holder.fields[1].ty_ref,
        CftSchemaTypeRef::Nullable(Box::new(CftSchemaTypeRef::Ref("Item".to_string())))
    );
    assert_eq!(holder.fields[2].ty, "[&Item]");
    assert_eq!(
        holder.fields[2].ty_ref,
        CftSchemaTypeRef::Array(Box::new(CftSchemaTypeRef::Ref("Item".to_string())))
    );
    assert_eq!(holder.fields[3].ty, "{string: &Item}");
    assert_eq!(
        holder.fields[3].ty_ref,
        CftSchemaTypeRef::Dict(
            Box::new(CftSchemaTypeRef::String),
            Box::new(CftSchemaTypeRef::Ref("Item".to_string())),
        )
    );
}

#[test]
fn ref_type_rejects_non_object_collection_and_singleton_targets() {
    for source in [
        "type Bad { value: &int; }",
        "enum Rarity { Common, } type Bad { value: &Rarity; }",
        "type Item { name: string; } type Bad { value: &[Item]; }",
        "type Item { name: string; } type Bad { value: &{string: Item}; }",
        "@singleton type Settings { value: string; } type Bad { value: &Settings; }",
    ] {
        let err = compile_one(source).expect_err(source);
        assert_has_code(&err, CftErrorCode::InvalidAnnotatedFieldType);
    }
}

#[test]
fn ref_type_rejects_expand_on_reference_field() {
    let err = compile_one(
        r#"
            type Stats { hp: int; }
            type Bad {
                @expand
                stats: &Stats;
            }
        "#,
    )
    .expect_err("@expand should only allow inline object fields");

    assert_has_code(&err, CftErrorCode::InvalidAnnotatedFieldType);
}
