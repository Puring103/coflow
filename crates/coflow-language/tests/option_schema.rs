#![allow(clippy::expect_used, clippy::needless_raw_string_hashes)]

use coflow_language::cft::{
    build_schema, parse_modules, CftFile, CftSchemaDefaultValue, CftValueType, ModuleId,
};

fn compile(
    source: &str,
) -> Result<coflow_language::cft::CftSchema, coflow_language::diagnostics::CftDiagnostics> {
    let modules = parse_modules([CftFile::from_source(ModuleId::from("main"), source)]);
    build_schema(&modules)
}

#[test]
fn legacy_nullable_type_and_null_default_are_rejected() {
    assert!(compile("table Item { value: int?; }").is_err());
    assert!(compile("table Item { value: int = null; }").is_err());
}

#[test]
fn deeply_nested_option_types_are_rejected_before_recursing() {
    use coflow_language::cft::syntax::parser::{parse_module_with_options, CftParseOptions};
    use coflow_language::limits::StructuralLimits;
    let source = format!(
        "table Item {{ value: {}int{}; }}",
        "Option<".repeat(300),
        ">".repeat(300)
    );
    let result = parse_module_with_options(
        &ModuleId::from("depth"),
        &source,
        CftParseOptions {
            structural_limits: StructuralLimits::new(4, 1000, 1000),
        },
    );
    assert!(result
        .expect_err("Option type must use depth budget")
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code
            == coflow_language::diagnostics::CftErrorCode::SyntaxStructureLimitExceeded));
}

#[test]
fn option_types_preserve_nested_defaults() {
    let schema = compile(
        r#"
            table Item {
                nested: Option<Option<int>> = Some(None);
            }
        "#,
    )
    .expect("nested Option syntax should compile");
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
