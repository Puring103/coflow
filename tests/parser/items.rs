use coflow::ast::{Expr, Item, Literal, StringKind};
use coflow::parser::ParseErrorKind;

use crate::common::{parse_error_kinds, parse_ok};

#[test]
fn empty_module_parses() {
    let module = parse_ok("");
    assert_eq!(module.items, []);
}

#[test]
fn top_level_config_with_int_literal_parses() {
    let module = parse_ok("base_damage = 10");
    assert_eq!(module.items.len(), 1);

    let Item::Config(config) = &module.items[0] else {
        panic!("expected config item");
    };
    assert_eq!(config.name.text, "base_damage");
    assert!(config.ty.is_none());
    assert!(matches!(
        &config.value,
        Expr::Literal(Literal::Int { raw, .. }) if raw == "10"
    ));
}

#[test]
fn typed_config_with_record_value_parses() {
    let module = parse_ok(
        r#"
sword: Weapon = {
  id: "sword",
  damage: 10,
}
"#,
    );

    let Item::Config(config) = &module.items[0] else {
        panic!("expected config item");
    };
    assert_eq!(config.name.text, "sword");
    assert!(config.ty.is_some());
    assert!(matches!(config.value, Expr::Record(_)));
}

#[test]
fn imports_with_optional_alias_parse() {
    let module = parse_ok("import common\nimport effects as fx");
    assert_eq!(module.items.len(), 2);

    let Item::Import(first) = &module.items[0] else {
        panic!("expected import");
    };
    assert_eq!(first.module.segments[0].text, "common");
    assert!(first.alias.is_none());

    let Item::Import(second) = &module.items[1] else {
        panic!("expected import");
    };
    assert_eq!(second.module.segments[0].text, "effects");
    assert_eq!(
        second.alias.as_ref().map(|ident| ident.text.as_str()),
        Some("fx")
    );
}

#[test]
fn top_level_var_and_local_var_parse() {
    let module = parse_ok("var runtime_cache = null\nlocal var scale: int = 2");
    assert_eq!(module.items.len(), 2);

    let Item::Var(first) = &module.items[0] else {
        panic!("expected var");
    };
    assert!(!first.local);
    assert_eq!(first.name.text, "runtime_cache");
    assert!(matches!(
        first.init,
        Some(Expr::Literal(Literal::Null { .. }))
    ));

    let Item::Var(second) = &module.items[1] else {
        panic!("expected local var");
    };
    assert!(second.local);
    assert_eq!(second.name.text, "scale");
    assert!(second.ty.is_some());
}

#[test]
fn functions_and_co_functions_parse() {
    let module = parse_ok(
        r#"
fn add(a: int, b) {
  return a + b
}

local co fn stream(count) {
  yield count
}
"#,
    );
    assert_eq!(module.items.len(), 2);

    let Item::Function(add) = &module.items[0] else {
        panic!("expected function");
    };
    assert_eq!(add.name.text, "add");
    assert!(!add.local);
    assert!(!add.co);
    assert_eq!(add.params.len(), 2);

    let Item::Function(stream) = &module.items[1] else {
        panic!("expected co function");
    };
    assert_eq!(stream.name.text, "stream");
    assert!(stream.local);
    assert!(stream.co);
}

#[test]
fn class_with_fields_defaults_and_validate_parses() {
    let module = parse_ok(
        r#"
local class Weapon {
  id: string
  damage: int = 10

  validate {
    if self.damage <= 0 {
      throw "damage must be positive"
    }
  }
}
"#,
    );

    let Item::Class(class) = &module.items[0] else {
        panic!("expected class");
    };
    assert!(class.local);
    assert_eq!(class.name.text, "Weapon");
    assert_eq!(class.fields.len(), 2);
    assert_eq!(class.fields[0].name.text, "id");
    assert!(class.fields[1].default.is_some());
    assert!(class.validate.is_some());
}

#[test]
fn enum_variants_parse() {
    let module = parse_ok("local enum Rarity { common rare epic }");
    let Item::Enum(enum_decl) = &module.items[0] else {
        panic!("expected enum");
    };
    assert!(enum_decl.local);
    assert_eq!(enum_decl.name.text, "Rarity");
    assert_eq!(
        enum_decl
            .variants
            .iter()
            .map(|variant| variant.text.as_str())
            .collect::<Vec<_>>(),
        vec!["common", "rare", "epic"]
    );
}

#[test]
fn top_level_string_config_preserves_string_kind() {
    let module = parse_ok("path = r\"C:\\game\\hero.png\"");
    let Item::Config(config) = &module.items[0] else {
        panic!("expected config");
    };
    assert!(matches!(
        &config.value,
        Expr::Literal(Literal::String(string)) if string.kind == StringKind::Raw
    ));
}

#[test]
fn rejects_top_level_runtime_statement() {
    let errors = parse_error_kinds("if true { print(true) }");
    assert!(errors.contains(&ParseErrorKind::ExpectedItem));
}

#[test]
fn rejects_local_config_declaration() {
    let output = coflow::parser::parse_module("local sword = {}");
    let errors = output
        .errors
        .iter()
        .map(|error| error.kind)
        .collect::<Vec<_>>();
    assert!(errors.contains(&ParseErrorKind::ExpectedItem));
    assert_eq!(
        output
            .module
            .expect("parser should return partial module")
            .items,
        []
    );
}

#[test]
fn rejects_co_without_fn() {
    let errors = parse_error_kinds("co stream() {}");
    assert!(errors.contains(&ParseErrorKind::ExpectedToken));
}

#[test]
fn rejects_import_without_module_name() {
    let errors = parse_error_kinds("import");
    assert!(errors.contains(&ParseErrorKind::ExpectedIdentifier));
}

#[test]
fn rejects_import_alias_without_name() {
    let errors = parse_error_kinds("import effects as");
    assert!(errors.contains(&ParseErrorKind::ExpectedIdentifier));
}

#[test]
fn rejects_class_field_without_type() {
    let errors = parse_error_kinds("class Weapon { id }");
    assert!(errors.contains(&ParseErrorKind::ExpectedType));
}
