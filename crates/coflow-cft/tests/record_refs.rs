#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::panic_in_result_fn,
    clippy::needless_raw_string_hashes
)]

mod common;
use coflow_cft::{CftValueType, TypeName};
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
    let fields = holder.own_fields().collect::<Vec<_>>();
    let item = TypeName::new("Item").unwrap();
    assert_eq!(fields[0].value_type.display_label(), "&Item");
    assert_eq!(fields[0].value_type, CftValueType::RecordRef(item.clone()));
    assert_eq!(fields[1].value_type.display_label(), "&Item?");
    assert_eq!(
        fields[1].value_type,
        CftValueType::Nullable(Box::new(CftValueType::RecordRef(item.clone())))
    );
    assert_eq!(fields[2].value_type.display_label(), "[&Item]");
    assert_eq!(
        fields[2].value_type,
        CftValueType::Array(Box::new(CftValueType::RecordRef(item.clone())))
    );
    assert_eq!(fields[3].value_type.display_label(), "{string: &Item}");
    assert_eq!(
        fields[3].value_type,
        CftValueType::Dict(
            Box::new(CftValueType::String),
            Box::new(CftValueType::RecordRef(item)),
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
        "@singleton type Settings { value: string; } type Bad { value: Settings; }",
        "@singleton type Settings { value: string; } type Bad { value: Settings?; }",
        "@singleton type Settings { value: string; } type Bad { value: [Settings]; }",
        "@singleton type Settings { value: string; } type Bad { value: {string: Settings}; }",
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
