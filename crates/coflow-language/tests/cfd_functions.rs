use coflow_language::cfd::{parse_cfd, CfdFormatSegment, CfdValue};

fn parse_function(source: &str) -> String {
    let source = format!("item: Rule {{ apply: {source} }}");
    let (ast, diagnostics) = parse_cfd(&source);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let CfdValue::Function(function) = &ast.records[0].fields[0].value else {
        panic!("expected function value");
    };
    function.source.clone()
}

#[test]
fn retains_function_source_and_finds_the_real_body_boundary() {
    for function in [
        "fn(value: int) -> int { value + 1 }",
        "fn() -> () { () }",
        "fn(value: Option<int>) -> Result<int, string> { Ok(value?) }",
        "fn(values: [int]) -> [int] { values }",
        "fn(values: {string: int}) -> {string: int} { values }",
        "fn(callback: fn(int) -> int) -> fn(int) -> int { callback }",
        "fn() -> int { fn() -> int { 1 }() }",
        "fn() -> string { \"a { brace } and \\\"quote\\\"\" }",
        "fn() -> int { # } is part of this comment\n 1 }",
    ] {
        assert_eq!(parse_function(function), function);
    }
}

#[test]
fn functions_are_values_inside_collections() {
    let source = "item: Rule { callbacks: [fn(x: int) -> int { x }, fn(x: int) -> int { x + 1 }] }";
    let (ast, diagnostics) = parse_cfd(source);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let CfdValue::Array(values, _) = &ast.records[0].fields[0].value else {
        panic!("expected array");
    };
    assert!(values
        .iter()
        .all(|value| matches!(value, CfdValue::Function(_))));
}

#[test]
fn reports_unterminated_function_structures() {
    for source in [
        "item: Rule { apply: fn(value: int -> int { value } }",
        "item: Rule { apply: fn(value: int) int { value } }",
        "item: Rule { apply: fn(value: int) -> {string: int { value } }",
        "item: Rule { apply: fn(value: int) -> int { \"unterminated } }",
        "item: Rule { apply: fn(value: int) -> int { value }",
    ] {
        let (_, diagnostics) = parse_cfd(source);
        assert!(!diagnostics.is_empty(), "expected an error for {source:?}");
    }
}

#[test]
fn rejects_qualified_function_type_names() {
    for source in [
        "item: Rule { apply: fn(value: Common::Input) -> int { 0 } }",
        "item: Rule { apply: fn(value: int) -> Common::Output { value } }",
    ] {
        let (_, diagnostics) = parse_cfd(source);
        assert!(!diagnostics.is_empty(), "expected an error for {source:?}");
    }
}

#[test]
fn validates_function_body_grammar_instead_of_skipping_it() {
    for body in [
        "+",
        "var value = ;",
        "if true { 1 } trailing",
        "match value { Some(item) item }",
        "for item values { item; }",
    ] {
        let source = format!("item: Rule {{ apply: fn() -> int {{ {body} }} }}");
        let (_, diagnostics) = parse_cfd(&source);
        assert!(!diagnostics.is_empty(), "expected an error for {body:?}");
    }
}

#[test]
fn function_identifiers_use_unicode_xid_rules() {
    let valid = "fn(\u{53C2}\u{6570}: int) -> int { var \u{7ED3}\u{679C}\u{0301} = \u{53C2}\u{6570} + 1; \u{7ED3}\u{679C}\u{0301} }";
    assert_eq!(parse_function(valid), valid);

    let invalid = "item: Rule { apply: fn() -> int { var \u{0301}value = 1; \u{0301}value } }";
    let (_, diagnostics) = parse_cfd(invalid);
    assert!(!diagnostics.is_empty());
}

#[test]
fn ordinary_strings_interpolate_and_the_removed_prefix_is_rejected() {
    let (ast, diagnostics) = parse_cfd(r#"item: Rule { label: "item {name}" }"#);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert!(matches!(
        &ast.records[0].fields[0].value,
        CfdValue::FormattedString(value)
            if value.segments.iter().any(|segment| matches!(segment, CfdFormatSegment::Reference(_)))
    ));

    let (_, diagnostics) = parse_cfd(r#"item: Rule { label: f"{name}" }"#);
    assert!(diagnostics
        .iter()
        .any(|diagnostic| diagnostic.message.contains("ordinary quotes")));
}
