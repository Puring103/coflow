#![allow(clippy::expect_used)]
use coflow_core::schema::{
    build_schema, parse_modules, CftDimensionInputs, CftFile, CftSchemaDefaultValue, CftValueType,
    ModuleId,
};
fn compile(
    source: &str,
) -> Result<coflow_core::schema::CftSchema, coflow_language::diagnostics::CftDiagnostics> {
    build_schema(
        &parse_modules([CftFile::from_source(ModuleId::from("main"), source)]),
        &CftDimensionInputs::default(),
    )
}
#[test]
fn optional_fields_accept_none_and_direct_values() {
    let schema = compile("table Item { a: int? = None; b: int? = 2; c: int? = 3; }")
        .expect("optional declarations");
    let item = schema.resolve_type("Item").expect("Item");
    assert_eq!(
        item.field("a").expect("a").value_type,
        CftValueType::Option(Box::new(CftValueType::Int))
    );
    assert_eq!(
        item.field("a").expect("a").default,
        Some(CftSchemaDefaultValue::OptionNone)
    );
    assert_eq!(
        item.field("c").expect("c").default,
        Some(CftSchemaDefaultValue::OptionSome(Box::new(
            CftSchemaDefaultValue::Int(3)
        )))
    );
}
#[test]
fn nested_optional_and_removed_generic_types_are_rejected() {
    for ty in ["int??", "(int?)?", "Option<int>", "Result<int, string>"] {
        assert!(
            compile(&format!("table Item {{ value: {ty}; }}")).is_err(),
            "{ty}"
        );
    }
    assert!(compile("table Item { value: int? = null; }").is_err());
    assert!(compile("table Item { value: int? = Some(2); }").is_err());
}
