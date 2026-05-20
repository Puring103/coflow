use coflow::ast::{Item, Stmt};
use coflow::parser::ParseErrorKind;

use crate::common::parse_errors;

#[test]
fn recovers_to_next_top_level_item_after_bad_config_value() {
    let output = coflow::parser::parse_module(
        r#"
bad = {
  id: "x",
  damage:
}

good = 1
"#,
    );

    assert!(output
        .errors
        .iter()
        .any(|error| error.kind == ParseErrorKind::ExpectedExpression));
    let module = output
        .module
        .expect("parser should recover a partial module");
    assert!(module
        .items
        .iter()
        .any(|item| { matches!(item, Item::Config(config) if config.name.text == "good") }));
}

#[test]
fn recovers_to_next_class_field_after_bad_field() {
    let output = coflow::parser::parse_module(
        r#"
class Weapon {
  id
  damage: int
}
"#,
    );

    assert!(output
        .errors
        .iter()
        .any(|error| error.kind == ParseErrorKind::ExpectedType));
    let module = output.module.expect("parser should recover class");
    let Item::Class(class) = &module.items[0] else {
        panic!("expected class");
    };
    assert!(class.fields.iter().any(|field| field.name.text == "damage"));
}

#[test]
fn recovers_to_next_statement_after_bad_statement() {
    let output = coflow::parser::parse_module(
        r#"
fn main() {
  a + = 1
  return 1
}
"#,
    );

    assert!(!output.errors.is_empty());
    let module = output.module.expect("parser should recover function");
    let Item::Function(func) = &module.items[0] else {
        panic!("expected function");
    };
    assert!(func
        .body
        .stmts
        .iter()
        .any(|stmt| matches!(stmt, Stmt::Return(_))));
}

#[test]
fn unclosed_delimiters_report_errors() {
    let errors = parse_errors("fn main() { call(1, 2");
    assert!(errors.iter().any(|error| matches!(
        error.kind,
        ParseErrorKind::ExpectedToken | ParseErrorKind::UnexpectedEof
    )));
}
