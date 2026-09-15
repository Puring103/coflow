#![allow(clippy::expect_used)]
use coflow_core::schema::{
    build_schema, parse_modules, CftDimensionInputs, CftFile, CftValueType, ModuleId, TypeName,
};

#[test]
fn imported_types_resolve_to_qualified_contract_names() {
    let modules = parse_modules([
        CftFile::from_source(
            ModuleId::from("common.cft"),
            "namespace common; data Position { x: int; }",
        ),
        CftFile::from_source(
            ModuleId::from("game.cft"),
            "namespace game; use common::Position; table Item { position: Position; }",
        ),
    ]);
    let schema = build_schema(&modules, &CftDimensionInputs::default()).expect("namespace imports");
    assert_eq!(
        schema
            .resolve_type("game::Item")
            .expect("Item")
            .field("position")
            .expect("field")
            .value_type,
        CftValueType::Object(TypeName::new("common::Position").expect("qualified name"))
    );
    assert!(schema.resolve_type("Item").is_none());
}

#[test]
fn same_short_names_in_distinct_namespaces_are_independent() {
    let modules = parse_modules([
        CftFile::from_source(ModuleId::from("one.cft"), "namespace one; table Item {}"),
        CftFile::from_source(ModuleId::from("two.cft"), "namespace two; table Item {}"),
    ]);
    let schema = build_schema(&modules, &CftDimensionInputs::default()).expect("independent names");
    assert!(schema.resolve_type("one::Item").is_some());
    assert!(schema.resolve_type("two::Item").is_some());
}

#[test]
fn duplicate_qualified_names_and_reserved_namespace_are_rejected() {
    let modules = parse_modules([
        CftFile::from_source(ModuleId::from("one.cft"), "namespace game; table Item {}"),
        CftFile::from_source(ModuleId::from("two.cft"), "namespace game; table Item {}"),
    ]);
    assert!(build_schema(&modules, &CftDimensionInputs::default()).is_err());
    let modules = parse_modules([CftFile::from_source(
        ModuleId::from("reserved.cft"),
        "namespace Coflow; table Item {}",
    )]);
    assert!(build_schema(&modules, &CftDimensionInputs::default()).is_err());
}
