#![allow(clippy::expect_used)]
use coflow_core::schema::{
    build_schema, dimension_record_type, parse_modules, CftDimensionInputs, CftFile, ModuleId,
};
#[test]
fn generated_names_are_unique_and_ambiguous_short_names_require_qualification() {
    let modules = parse_modules([CftFile::from_source(
        ModuleId::from("main"),
        "table A_B { @localized c: string; } table A { @localized B_c: string; }",
    )]);
    let dimensions =
        CftDimensionInputs::try_new([("language", vec!["zh".into()])]).expect("dimensions");
    let schema = build_schema(&modules, &dimensions).expect("canonical names do not collide");
    let first = dimension_record_type("language", "A_B", "c");
    let second = dimension_record_type("language", "A", "B_c");
    assert_ne!(first, second);
    assert!(schema.resolve_type(&first).is_some());
    assert!(schema.resolve_type(&second).is_some());
    assert!(schema.resolve_record_type_name("A_B_c_language").is_err());
    assert_eq!(
        schema.resolve_record_type_name(&first).expect("qualified"),
        first
    );
}
#[test]
fn reserved_namespace_covers_all_declaration_kinds() {
    for source in [
        "namespace Coflow; table Item {}",
        "namespace Coflow; enum E { One }",
        "namespace Coflow; type T = string;",
    ] {
        let modules = parse_modules([CftFile::from_source(ModuleId::from("main"), source)]);
        assert!(build_schema(&modules, &CftDimensionInputs::default()).is_err());
    }
}
