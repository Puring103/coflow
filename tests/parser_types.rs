use coflow::ast::{Item, TypeExpr};
use coflow::parser::ParseErrorKind;

mod common;
use common::{parse_error_kinds, parse_ok};

fn parse_config_type(source: &str) -> TypeExpr {
    let module = parse_ok(source);
    let Item::Config(config) = &module.items[0] else {
        panic!("expected config");
    };
    config.ty.clone().expect("expected config type")
}

#[test]
fn parses_simple_type_name() {
    let ty = parse_config_type("value: int = 1");
    assert!(matches!(ty, TypeExpr::Name(path) if path.segments[0].text == "int"));
}

#[test]
fn parses_path_type_name() {
    let ty = parse_config_type("value: common.Weapon = {}");
    assert!(matches!(ty, TypeExpr::Name(path) if path.segments.len() == 2));
}

#[test]
fn parses_array_type() {
    let ty = parse_config_type("values: [string] = []");
    assert!(matches!(ty, TypeExpr::Array { .. }));
}

#[test]
fn parses_dict_type() {
    let ty = parse_config_type("scores: [string: int] = {}");
    assert!(matches!(ty, TypeExpr::Dict { .. }));
}

#[test]
fn parses_nested_type_expressions() {
    let ty = parse_config_type("value: [string: [common.Weapon]] = {}");
    let TypeExpr::Dict { key, value, .. } = ty else {
        panic!("expected dict type");
    };
    assert!(matches!(*key, TypeExpr::Name(_)));
    assert!(matches!(*value, TypeExpr::Array { .. }));
}

#[test]
fn rejects_empty_bracket_type() {
    let errors = parse_error_kinds("value: [] = []");
    assert!(errors.contains(&ParseErrorKind::ExpectedType));
}

#[test]
fn rejects_dict_type_without_value_type() {
    let errors = parse_error_kinds("value: [string:] = {}");
    assert!(errors.contains(&ParseErrorKind::ExpectedType));
}

#[test]
fn rejects_missing_type_after_colon() {
    let errors = parse_error_kinds("value: = 1");
    assert!(errors.contains(&ParseErrorKind::ExpectedType));
}

#[test]
fn rejects_unclosed_type_bracket() {
    let errors = parse_error_kinds("value: [string: int = {}");
    assert!(errors.contains(&ParseErrorKind::ExpectedToken));
}
