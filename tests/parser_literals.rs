use coflow::ast::{Expr, Item, Literal, RecordKey, StringKind};
use coflow::parser::ParseErrorKind;

mod common;
use common::{parse_error_kinds, parse_ok};

fn parse_config_value(source: &str) -> Expr {
    let module = parse_ok(source);
    let Item::Config(config) = &module.items[0] else {
        panic!("expected config");
    };
    config.value.clone()
}

#[test]
fn parses_primitive_literals() {
    let cases = [
        ("v = 1", "int"),
        ("v = 1.5e-3", "float"),
        ("v = \"hero\"", "string"),
        ("v = true", "bool"),
        ("v = false", "bool"),
        ("v = null", "null"),
    ];

    for (source, kind) in cases {
        let expr = parse_config_value(source);
        match kind {
            "int" => assert!(matches!(expr, Expr::Literal(Literal::Int { .. }))),
            "float" => assert!(matches!(expr, Expr::Literal(Literal::Float { .. }))),
            "string" => assert!(matches!(expr, Expr::Literal(Literal::String(_)))),
            "bool" => assert!(matches!(expr, Expr::Literal(Literal::Bool { .. }))),
            "null" => assert!(matches!(expr, Expr::Literal(Literal::Null { .. }))),
            _ => unreachable!(),
        }
    }
}

#[test]
fn parses_all_string_literal_kinds() {
    let cases = [
        ("v = \"hero\"", StringKind::Normal),
        (r#"v = r"C:\game""#, StringKind::Raw),
        ("v = \"\"\"line\nline\"\"\"", StringKind::Multiline),
        ("v = r\"\"\"line\\n\nline\"\"\"", StringKind::RawMultiline),
    ];

    for (source, expected) in cases {
        let expr = parse_config_value(source);
        assert!(matches!(
            expr,
            Expr::Literal(Literal::String(string)) if string.kind == expected
        ));
    }
}

#[test]
fn parses_arrays_with_optional_trailing_comma() {
    let expr = parse_config_value("v = [1, 2, 3,]");
    let Expr::Array(array) = expr else {
        panic!("expected array");
    };
    assert_eq!(array.elements.len(), 3);
}

#[test]
fn parses_records_with_identifier_and_string_keys_and_trailing_comma() {
    let expr = parse_config_value(
        r#"
v = {
  id: "sword",
  "damage": 10,
}
"#,
    );
    let Expr::Record(record) = expr else {
        panic!("expected record");
    };
    assert_eq!(record.entries.len(), 2);
    assert!(matches!(record.entries[0].key, RecordKey::Ident(_)));
    assert!(matches!(record.entries[1].key, RecordKey::String(_)));
}

#[test]
fn parses_deeply_nested_record_array_values() {
    let expr = parse_config_value(
        r#"
value = {
  effects: [
    { id: "burn", duration: 3.5, },
    { id: "push", vector: [0, 1, 0,], },
  ],
}
"#,
    );
    let Expr::Record(record) = expr else {
        panic!("expected record");
    };
    assert_eq!(record.entries.len(), 1);
}

#[test]
fn parses_function_values_with_trailing_param_comma() {
    let expr = parse_config_value(
        r#"
apply = fn(caster, target,) {
  target.hp -= 10
}
"#,
    );
    let Expr::Fn(func) = expr else {
        panic!("expected function expression");
    };
    assert!(!func.co);
    assert_eq!(func.params.len(), 2);
}

#[test]
fn parses_co_function_values() {
    let expr = parse_config_value(
        r#"
stream = co fn(count,) {
  yield count
}
"#,
    );
    let Expr::Fn(func) = expr else {
        panic!("expected co function expression");
    };
    assert!(func.co);
    assert_eq!(func.params.len(), 1);
}

#[test]
fn rejects_record_entry_without_colon() {
    let errors = parse_error_kinds("v = { id }");
    assert!(errors.contains(&ParseErrorKind::ExpectedToken));
}

#[test]
fn rejects_record_entry_without_value() {
    let errors = parse_error_kinds("v = { id: }");
    assert!(errors.contains(&ParseErrorKind::ExpectedExpression));
}

#[test]
fn rejects_array_with_empty_entry_before_comma() {
    let errors = parse_error_kinds("v = [1, , 2]");
    assert!(errors.contains(&ParseErrorKind::ExpectedExpression));
}
