use coflow_language::{
    build_schema, parse_modules, CftDimensionInputs, CftFile, CftSchemaDefaultValue, CftValueType,
    ModuleId,
};

fn compile(source: &str) -> Result<coflow_language::CftSchema, coflow_language::CftDiagnostics> {
    let modules = parse_modules([CftFile::from_source(ModuleId::from("main"), source)]);
    build_schema(&modules, &CftDimensionInputs::default())
}

#[test]
fn legacy_nullable_type_and_null_default_are_rejected() {
    assert!(compile("type Item { value: int?; }").is_err());
    assert!(compile("type Item { value: int = null; }").is_err());
}

#[test]
fn option_and_result_types_preserve_nested_defaults() {
    let schema = compile(
        r#"
            type Item {
                nested: Option<Option<int>> = Some(None);
                outcome: Result<Option<int>, string> = Ok(Some(3));
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
    let outcome = item.field("outcome").expect("outcome field");
    assert_eq!(
        outcome.default,
        Some(CftSchemaDefaultValue::ResultOk(Box::new(
            CftSchemaDefaultValue::OptionSome(Box::new(CftSchemaDefaultValue::Int(3)))
        )))
    );
}
