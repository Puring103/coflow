#![allow(clippy::expect_used)]
use coflow_language::cft::{build_schema, parse_modules, CftFile, ModuleId};

#[test]
fn dimension_fields_use_independent_coordinates_without_auxiliary_type_names() {
    let modules = parse_modules([CftFile::from_source(
        ModuleId::from("main"),
        "table A_B { @localized c: string; } table A { @localized B_c: string; }",
    )]);
    let schema = build_schema(&modules).expect("schema");
    let dimension = schema.resolve_dimension("language").expect("dimension");
    assert_eq!(dimension.fields.len(), 2);
    assert!(schema.field("A_B", "c").is_some());
    assert!(schema.field("A", "B_c").is_some());
}

#[test]
fn declarations_can_use_names_previously_reserved_for_auxiliary_records() {
    for source in [
        "table __coflow_language_Item_name { value: string; }",
        "enum __coflow_language_Item_name { One }",
        "type __coflow_language_Item_name = string;",
    ] {
        let modules = parse_modules([CftFile::from_source(ModuleId::from("main"), source)]);
        build_schema(&modules).expect("schema");
    }
}
