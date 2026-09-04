use coflow_language::{
    build_schema, parse_modules, CftConstValue, CftErrorCode, CftFile, CftSchemaDefaultValue,
    ModuleId,
};

fn compile(source: &str) -> Result<coflow_language::CftSchema, coflow_language::CftDiagnostics> {
    let modules = parse_modules([CftFile::from_source(ModuleId::from("main"), source)]);
    build_schema(&modules, &Default::default())
}

#[test]
fn supports_runtime_field_defaults() {
    let schema = compile(
        r#"
@flag enum Permission { Read = 1, Write = 2 }
const BRACES: string = "{{ready}}";
abstract type Effect { amount: int; }
type Damage: Effect {}
@description("literal {{braces}}")
type Rule {
  name: string;
  label: string = "rule {name}";
  braces: string = "{{ready}}";
  permissions: Permission = Permission::Read | Permission::Write;
  effect: Effect = Damage { amount: 2 };
  apply: fn(value: int) -> int = fn(input: int) -> int { input + 1 };
}
"#,
    )
    .expect("runtime defaults compile");
    let rule = schema.resolve_type("Rule").expect("Rule");

    assert!(matches!(
        rule.field("label").and_then(|field| field.default.as_ref()),
        Some(CftSchemaDefaultValue::FormattedString(source)) if source == "\"rule {name}\""
    ));
    assert!(matches!(
        rule.field("braces").and_then(|field| field.default.as_ref()),
        Some(CftSchemaDefaultValue::String(value)) if value == "{ready}"
    ));
    assert!(matches!(
        rule.field("permissions").and_then(|field| field.default.as_ref()),
        Some(CftSchemaDefaultValue::Enum { value: 3, .. })
    ));
    assert!(matches!(
        rule.field("effect").and_then(|field| field.default.as_ref()),
        Some(CftSchemaDefaultValue::Object { type_name, .. }) if type_name.as_str() == "Damage"
    ));
    assert!(matches!(
        rule.field("apply").and_then(|field| field.default.as_ref()),
        Some(CftSchemaDefaultValue::Function(source)) if source.contains("input + 1")
    ));
    assert!(matches!(
        schema.resolve_const("BRACES").map(|value| &value.value),
        Some(CftConstValue::String(value)) if value == "{ready}"
    ));
}

#[test]
fn rejects_removed_formatted_string_prefix() {
    for source in [
        r#"type Rule { label: string = f"{id}"; }"#,
        r#"type Rule { value: int; check { value > 0: f"invalid {value}"; } }"#,
    ] {
        let diagnostics = compile(source).expect_err("the f prefix must be rejected");
        assert!(diagnostics.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == CftErrorCode::UnexpectedCharacter
                && diagnostic.message.contains("ordinary quotes")
        }));
    }
}

#[test]
fn rejects_invalid_function_defaults() {
    let diagnostics = compile(
        "type Rule { apply: fn(value: int) -> int = fn(value: string) -> int { 1 }; }",
    )
    .expect_err("a mismatched default signature must fail");
    assert!(diagnostics.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == CftErrorCode::InvalidConstValue
            && diagnostic.message.contains("expected `fn(value: int) -> int`")
    }));

    let diagnostics = compile(
        "@Host @singleton type Services { apply: fn(value: int) -> int = fn(value: int) -> int { value }; }",
    )
    .expect_err("host functions cannot have default implementations");
    assert!(diagnostics.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == CftErrorCode::InvalidDefaultExpression
            && diagnostic.message.contains("@Host function fields")
    }));

    let diagnostics = compile(
        "type ServiceBase { apply: fn(value: int) -> int = fn(value: int) -> int { value }; } @Host @singleton type Services: ServiceBase {}",
    )
    .expect_err("host functions cannot inherit default implementations");
    assert!(diagnostics.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == CftErrorCode::InvalidDefaultExpression
            && diagnostic.message.contains("inherited by @Host type `Services`")
    }));

    let diagnostics = compile(
        "type Rule { callbacks: [fn(value: int) -> int] = [fn(value: int) -> int { value }]; }",
    )
    .expect_err("function defaults cannot be nested in collections");
    assert!(diagnostics.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == CftErrorCode::InvalidDefaultExpression
            && diagnostic.message.contains("directly on function fields")
    }));
}

#[test]
fn rejects_invalid_flag_default_expressions() {
    let diagnostics = compile(
        "enum Mode { A = 1, B = 2 } type Rule { mode: Mode = Mode::A | Mode::B; }",
    )
    .expect_err("bit expressions require a flag enum");
    assert!(diagnostics
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == CftErrorCode::InvalidConstValue));

    let diagnostics = compile(
        "@flag enum Permission { Read = 1, Write = 2 } type Rule { permissions: Permission = 8; }",
    )
    .expect_err("flag defaults cannot contain undeclared bits");
    assert!(diagnostics.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == CftErrorCode::InvalidConstValue
            && diagnostic.message.contains("undeclared bits")
    }));
}
