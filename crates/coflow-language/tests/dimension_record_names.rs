#![allow(clippy::expect_used)]

use coflow_language::cft::{build_schema, parse_modules, CftDimensionInputs, CftFile, ModuleId};

#[test]
fn dimension_record_names_reject_ambiguous_field_coordinates() {
    let modules = parse_modules([CftFile::from_source(
        ModuleId::from("main"),
        "type A_B { @localized c: string; } type A { @localized B_c: string; }",
    )]);
    let dimensions =
        CftDimensionInputs::try_new([("language", vec!["zh".into()])]).expect("dimensions");
    let diagnostics = build_schema(&modules, &dimensions).expect_err("ambiguous auxiliary name");
    assert!(diagnostics
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.message.contains("__coflow_language_A_B_c")));
}

#[test]
fn dimension_record_names_reserve_system_type_prefix() {
    let modules = parse_modules([CftFile::from_source(
        ModuleId::from("main"),
        "type __coflow_language_Item_name { value: string; }",
    )]);
    let diagnostics =
        build_schema(&modules, &CftDimensionInputs::default()).expect_err("system type prefix");
    assert!(diagnostics
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.message.contains("__coflow_")));
}

#[test]
fn dimension_record_names_reserve_enum_and_alias_names() {
    for source in [
        "enum __coflow_language_Item_name { One }",
        "type __coflow_language_Item_name = string;",
    ] {
        let modules = parse_modules([CftFile::from_source(ModuleId::from("main"), source)]);
        assert!(
            build_schema(&modules, &CftDimensionInputs::default()).is_err(),
            "{source}"
        );
    }
}
