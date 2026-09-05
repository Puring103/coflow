use coflow_language::cft::syntax::parser::parse_module;
use coflow_language::cft::{
    build_schema, parse_modules, CftDimensionInputs, CftFile, CftValueType, ModuleId, TypeName,
};
use coflow_language::diagnostics::CftErrorCode;

#[test]
fn namespace_and_use_are_not_language_syntax() {
    for source in [
        "namespace game; type Item {}",
        "use common::Item; type Item {}",
        "use common::Item as Imported; type Item {}",
    ] {
        let diagnostics = parse_module(&ModuleId::from("invalid.cft"), source)
            .expect_err("removed declarations must be rejected");
        assert!(diagnostics
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == CftErrorCode::InvalidTopLevelItem));
    }
}

#[test]
fn declarations_are_project_global_across_files() {
    let modules = parse_modules([
        CftFile::from_source(
            ModuleId::from("common.cft"),
            "type Position { x: int; } enum Quality { Good }",
        ),
        CftFile::from_source(
            ModuleId::from("item.cft"),
            "type Item { position: Position; quality: Quality = Quality::Good; }",
        ),
    ]);
    let schema = build_schema(&modules, &CftDimensionInputs::default())
        .expect("short names should resolve across files");
    assert_eq!(
        schema
            .resolve_type("Item")
            .expect("Item")
            .field("position")
            .expect("position")
            .value_type,
        CftValueType::Object(TypeName::new("Position").expect("short name"))
    );
}

#[test]
fn project_global_declarations_must_be_unique() {
    let modules = parse_modules([
        CftFile::from_source(ModuleId::from("one.cft"), "type Item {}"),
        CftFile::from_source(ModuleId::from("two.cft"), "type Item {}"),
    ]);
    let diagnostics = build_schema(&modules, &CftDimensionInputs::default())
        .expect_err("duplicate global name must fail");
    assert!(diagnostics
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == CftErrorCode::DuplicateGlobalName));
}

#[test]
fn qualified_type_names_are_rejected_but_static_paths_remain_valid() {
    for source in [
        "type Base {} type Item: group::Base {}",
        "type Item { value: group::Value; }",
        "type Item { check { self is group::Item; } }",
    ] {
        assert!(parse_module(&ModuleId::from("invalid.cft"), source).is_err());
    }

    let source = "enum Quality { Good } type Item { quality: Quality = Quality::Good; }";
    assert!(parse_module(&ModuleId::from("valid.cft"), source).is_ok());
}
