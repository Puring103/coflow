use coflow::hir::{HirExpr, HirGlobal, HirStmt, Ty, Value};
use coflow::sema::{analyze_source, Diagnostic, SemaErrorKind};

fn sema_kinds(source: &str) -> Vec<SemaErrorKind> {
    analyze_source(source)
        .diagnostics
        .into_iter()
        .filter_map(|diagnostic| match diagnostic {
            Diagnostic::Sema(kind, _) => Some(kind),
            _ => None,
        })
        .collect()
}

fn has_error(source: &str, kind: SemaErrorKind) -> bool {
    sema_kinds(source).contains(&kind)
}

#[test]
fn collects_enum_values_and_evaluates_config() {
    let output = analyze_source(
        r#"
enum Rarity { common, rare = 5, epic }
base = 2;
total: int = base + 40;
"#,
    );

    assert_eq!(output.diagnostics, []);
    assert_eq!(output.hir.enums[0].variants[0].value, 0);
    assert_eq!(output.hir.enums[0].variants[1].value, 5);
    assert_eq!(output.hir.enums[0].variants[2].value, 6);

    let (_, value) = output
        .hir
        .config_values
        .iter()
        .find(|(id, _)| matches!(&output.hir.globals[id.0], HirGlobal::Config { name, .. } if name == "total"))
        .expect("total config should evaluate");
    assert_eq!(value, &Value::Int(42));
}

#[test]
fn reports_duplicate_top_level_and_fields() {
    assert!(has_error(
        r#"
class Weapon { id: string id: int }
Weapon = {};
"#,
        SemaErrorKind::DuplicateTopLevel
    ));
    assert!(has_error(
        r#"
class Weapon { id: string id: int }
"#,
        SemaErrorKind::DuplicateField
    ));
}

#[test]
fn resolves_forward_function_signature_for_calls() {
    let output = analyze_source(
        r#"
fn main() -> int {
  return add(1, 2);
}

fn add(a: int, b: int) -> int => a + b
"#,
    );
    assert_eq!(output.diagnostics, []);

    let main_fn = output
        .hir
        .functions
        .iter()
        .find(|function| {
            matches!(
                function.signature,
                Ty::FunctionSig {
                    ref params,
                    ref return_ty
                } if params.is_empty() && **return_ty == Ty::Int
            )
        })
        .expect("main function should exist");
    assert!(matches!(
        main_fn.body.last(),
        Some(HirStmt::Return {
            value: Some(HirExpr::TypeGuard { .. }),
            ..
        })
    ));
}

#[test]
fn reports_type_mismatch_and_unknown_names() {
    assert!(has_error("var x: int = \"s\";", SemaErrorKind::TypeMismatch));
    assert!(has_error(
        r#"
fn main(value: string) {
  var hp: int = 1;
  hp = value;
}
"#,
        SemaErrorKind::TypeMismatch
    ));
    assert!(has_error(
        "fn main() { missing(); }",
        SemaErrorKind::UndefinedName
    ));
    assert!(has_error(
        "fn spawn(hp: int = \"bad\") {}",
        SemaErrorKind::TypeMismatch
    ));
    assert!(has_error(
        "var stream = iter fn() -> int { yield 1; };",
        SemaErrorKind::TypeMismatch
    ));
    assert!(has_error(
        "enum Rarity { common }\nfn main() { Rarity.common = 1; }",
        SemaErrorKind::AssignToReadonly
    ));
}

#[test]
fn assignment_to_typed_locations_inserts_runtime_guards() {
    let output = analyze_source(
        r#"
fn main(value: any) {
  var hp: int = 1;
  hp = value;
}
"#,
    );
    assert_eq!(output.diagnostics, []);
    let body = &output.hir.functions[0].body;
    assert!(body.iter().any(|stmt| matches!(
        stmt,
        HirStmt::Assign {
            value: HirExpr::TypeGuard { ty: Ty::Int, .. },
            ..
        }
    )));
}

#[test]
fn function_values_use_expected_signature_for_unannotated_params() {
    let output = analyze_source(
        r#"
var double: fn(int) -> int = fn(x) -> int => x;
"#,
    );
    assert_eq!(output.diagnostics, []);
    let function = output.hir.functions.first().expect("closure function");
    assert_eq!(function.params[0].ty, Some(Ty::Int));
}

#[test]
fn function_top_type_can_narrow_to_named_signature() {
    let output = analyze_source(
        r#"
var callback: fn(any) -> any = print;
"#,
    );
    assert_eq!(output.diagnostics, []);
    let init = output
        .hir
        .globals
        .iter()
        .find_map(|global| match global {
            HirGlobal::Var { name, init, .. } if name == "callback" => init.as_ref(),
            _ => None,
        })
        .expect("callback init");
    assert!(matches!(init, HirExpr::TypeGuard { .. }));
}

#[test]
fn lowers_until_for_range_and_null_coalesce_assign() {
    let output = analyze_source(
        r#"
fn main() {
  var x = null;
  x ??= 1;
  until x > 10 {
    x += 1;
  }
  for i in 0..=3 {
    x += i;
  }
}
"#,
    );
    assert_eq!(output.diagnostics, []);
    let body = &output.hir.functions[0].body;
    assert!(body.iter().any(|stmt| matches!(stmt, HirStmt::If { .. })));
    assert!(body.iter().any(|stmt| matches!(
        stmt,
        HirStmt::While {
            cond: HirExpr::Unary { .. },
            ..
        }
    )));
    assert!(body.iter().any(|stmt| matches!(
        stmt,
        HirStmt::ForRange {
            inclusive: true,
            ..
        }
    )));
}

#[test]
fn captures_outer_locals_in_closures() {
    let output = analyze_source(
        r#"
fn outer() {
  var count = 0;
  fn inc() {
    count += 1;
  }
}
"#,
    );
    assert_eq!(output.diagnostics, []);
    let outer = &output.hir.functions[0];
    assert!(outer.locals.iter().any(|local| local.is_captured));
    let inner = output
        .hir
        .functions
        .iter()
        .find(|function| !function.upvalues.is_empty())
        .expect("inner function should capture");
    assert_eq!(inner.upvalues.len(), 1);
}

#[test]
fn evaluates_class_defaults_and_checks() {
    let output = analyze_source(
        r#"
class Range {
  min: int
  max: int = 10
  check {
    self.min <= self.max => "bad range"
  }
}

limits: Range = { min: 1 };
"#,
    );
    assert_eq!(output.diagnostics, []);
    let (_, value) = output.hir.config_values.first().expect("config value");
    let Value::Object { fields, .. } = value else {
        panic!("expected object value");
    };
    assert!(fields
        .iter()
        .any(|(name, value)| name == "max" && value == &Value::Int(10)));
}

#[test]
fn class_defaults_and_checks_can_depend_on_configs() {
    let output = analyze_source(
        r#"
class Range {
  min: int
  max: int = default_max
  check {
    self.min <= default_max => "bad range"
  }
}

limits: Range = { min: 1 };
default_max = 10;
"#,
    );
    assert_eq!(output.diagnostics, []);
    let (_, value) = output
        .hir
        .config_values
        .iter()
        .find(|(id, _)| matches!(&output.hir.globals[id.0], HirGlobal::Config { name, .. } if name == "limits"))
        .expect("limits config value");
    let Value::Object { fields, .. } = value else {
        panic!("expected object value");
    };
    assert!(fields
        .iter()
        .any(|(name, value)| name == "max" && value == &Value::Int(10)));
}

#[test]
fn reports_config_dependency_errors() {
    assert!(has_error(
        r#"
var runtime = 1;
value = runtime;
"#,
        SemaErrorKind::ConfigDependsOnVar
    ));
    assert!(has_error(
        r#"
a = b;
b = a;
"#,
        SemaErrorKind::ConfigCircularDependency
    ));
}
