#![allow(clippy::expect_used)]

use coflow_core::schema::{
    build_schema, parse_modules, CftConstValue, CftFile, CftSchemaDefaultValue, ModuleId,
};
use coflow_language::diagnostics::CftErrorCode;

fn compile(
    source: &str,
) -> Result<coflow_core::schema::CftSchema, coflow_language::diagnostics::CftDiagnostics> {
    let modules = parse_modules([CftFile::from_source(ModuleId::from("main"), source)]);
    build_schema(&modules)
}

#[test]
fn supports_runtime_field_defaults() {
    let schema = compile(
        r#"
@flag enum Permission { Read = 1, Write = 2 }
const BRACES: string = "{{ready}}";
abstract data Effect { amount: int; }
data Damage: Effect {}
@description("literal {{braces}}")
table Rule {
  name: string;
  label: fstring = f"rule {self.name}";
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
        Some(CftSchemaDefaultValue::FormattedString(source)) if source.source == "f\"rule {self.name}\""
    ));
    assert!(matches!(
        rule.field("braces").and_then(|field| field.default.as_ref()),
        Some(CftSchemaDefaultValue::String(value)) if value == "{{ready}}"
    ));
    assert!(matches!(
        rule.field("permissions")
            .and_then(|field| field.default.as_ref()),
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
        Some(CftConstValue::String(value)) if value == "{{ready}}"
    ));
}

#[test]
fn plain_strings_do_not_become_templates() {
    assert!(compile(r#"table Rule { label: fstring = "{self.id}"; }"#).is_err());
    let schema = compile(r#"table Rule { label: string = "{self.id}"; }"#).expect("literal braces");
    assert!(
        matches!(schema.resolve_type("Rule").expect("Rule").field("label").and_then(|f| f.default.as_ref()),
        Some(CftSchemaDefaultValue::String(value)) if value == "{self.id}")
    );
}

#[test]
fn rejects_invalid_function_defaults() {
    let diagnostics =
        compile("table Rule { apply: fn(value: int) -> int = fn(value: string) -> int { 1 }; }")
            .expect_err("a mismatched default signature must fail");
    assert!(diagnostics.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == CftErrorCode::InvalidConstValue
            && diagnostic
                .message
                .contains("expected `fn(value: int) -> int`")
    }));

    let diagnostics = compile(
        "@Host singleton Services { apply: fn(value: int) -> int = fn(value: int) -> int { value }; }",
    )
    .expect_err("host functions cannot have default implementations");
    assert!(diagnostics.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == CftErrorCode::InvalidDefaultExpression
            && diagnostic.message.contains("@Host function fields")
    }));

    let diagnostics = compile(
        "table ServiceBase { apply: fn(value: int) -> int = fn(value: int) -> int { value }; } @Host singleton Services: ServiceBase {}",
    )
    .expect_err("host functions cannot inherit default implementations");
    assert!(diagnostics.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == CftErrorCode::InvalidDefaultExpression
            && diagnostic
                .message
                .contains("inherited by @Host type `Services`")
    }));

    compile(
        "table Rule { callbacks: [fn(value: int) -> int] = [fn(value: int) -> int { value }]; }",
    )
    .expect("function values in collections preserve declared signatures");
}

#[test]
fn rejects_invalid_flag_default_expressions() {
    let diagnostics =
        compile("enum Mode { A = 1, B = 2 } table Rule { mode: Mode = Mode::A | Mode::B; }")
            .expect_err("bit expressions require a flag enum");
    assert!(diagnostics
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == CftErrorCode::InvalidConstValue));

    let diagnostics = compile(
        "@flag enum Permission { Read = 1, Write = 2 } table Rule { permissions: Permission = 8; }",
    )
    .expect_err("flag defaults cannot contain undeclared bits");
    assert!(diagnostics.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == CftErrorCode::InvalidConstValue
            && diagnostic.message.contains("undeclared bits")
    }));
}
