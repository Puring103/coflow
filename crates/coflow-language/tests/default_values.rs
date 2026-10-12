#![allow(clippy::expect_used)]

use coflow_language::cft::{
    build_schema, parse_modules, CftConstValue, CftFile, CftSchemaDefaultValue, ModuleId,
};
use coflow_language::diagnostics::CftErrorCode;

fn compile(
    source: &str,
) -> Result<coflow_language::cft::CftSchema, coflow_language::diagnostics::CftDiagnostics> {
    let modules = parse_modules([CftFile::from_source(ModuleId::from("main"), source)]);
    build_schema(&modules)
}

#[test]
fn supports_data_field_defaults() {
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
}
"#,
    )
    .expect("data defaults compile");
    let rule = schema.resolve_type("Rule").expect("Rule");

    assert!(matches!(
        rule.field("label").and_then(|field| field.default.as_ref()),
        Some(CftSchemaDefaultValue::FormattedString(source)) if source == "f\"rule {self.name}\""
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
        schema.resolve_const("BRACES").map(|value| &value.value),
        Some(CftConstValue::String(value)) if value == "{{ready}}"
    ));
}

#[test]
fn ordinary_string_braces_are_literal() {
    let schema =
        compile(r#"table Rule { label: string = "{self.name}"; }"#).expect("literal braces");
    let rule = schema.resolve_type("Rule").expect("Rule");
    assert!(
        matches!(rule.field("label").and_then(|field| field.default.as_ref()), Some(CftSchemaDefaultValue::String(value)) if value == "{self.name}")
    );
}

#[test]
fn formatted_defaults_only_accept_field_paths() {
    for source in [
        r#"table Rule { name: string; label: fstring = f"{self.name()}"; }"#,
        r#"table Rule { price: int; label: fstring = f"{self.price + 1}"; }"#,
        r#"table Rule { price: int; label: fstring = f"{self.price:000}"; }"#,
    ] {
        let diagnostics = compile(source).expect_err("data interpolation must reject expressions");
        assert!(diagnostics
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == CftErrorCode::InvalidDefaultExpression));
    }
}

#[test]
fn function_types_and_defaults_are_rejected() {
    for source in [
        "table Rule { apply: fn(value: int) -> int; }",
        "table Rule { apply: int = fn(value: int) -> int { value }; }",
    ] {
        assert!(compile(source).is_err(), "{source}");
    }
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

#[test]
fn check_messages_only_interpolate_paths() {
    compile(r#"table Rule { price: int; check { price >= 0 : f"bad {price}"; } }"#)
        .expect("scalar field interpolation");
    for expression in [
        "price + 1",
        "price.abs()",
        "price:000",
        "-price",
        "len(price)",
    ] {
        let source = format!(
            "table Rule {{ price: int; check {{ price >= 0 : f\"bad {{{expression}}}\"; }} }}"
        );
        assert!(compile(&source).is_err(), "{expression}");
    }
}
