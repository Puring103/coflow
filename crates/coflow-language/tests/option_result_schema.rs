use coflow_language::cft::{
    build_schema, parse_modules, CftDimensionInputs, CftFile, CftSchemaDefaultValue, CftValueType,
    ModuleId,
};

fn compile(source: &str) -> Result<coflow_language::cft::CftSchema, coflow_language::diagnostics::CftDiagnostics> {
    let modules = parse_modules([CftFile::from_source(ModuleId::from("main"), source)]);
    build_schema(&modules, &CftDimensionInputs::default())
}

#[test]
fn legacy_nullable_type_and_null_default_are_rejected() {
    assert!(compile("type Item { value: int?; }").is_err());
    assert!(compile("type Item { value: int = null; }").is_err());
}

#[test]
fn option_types_preserve_nested_defaults() {
    let schema = compile(
        r#"
            type Item {
                nested: Option<Option<int>> = Some(None);
            }
        "#,
    )
    .expect("new Option and Result syntax should compile");
    let item = schema.resolve_type("Item").expect("Item type");
    let nested = item.field("nested").expect("nested field");
    assert_eq!(
        nested.value_type,
        CftValueType::Option(Box::new(CftValueType::Option(Box::new(CftValueType::Int))))
    );
    assert_eq!(
        nested.default,
        Some(CftSchemaDefaultValue::OptionSome(Box::new(
            CftSchemaDefaultValue::OptionNone
        )))
    );
}
