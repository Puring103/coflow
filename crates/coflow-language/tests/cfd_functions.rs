use coflow_language::cfd::{parse_cfd, CfdValue};

#[test]
fn stores_function_bodies_and_tracks_balanced_delimiters() {
    for function in [
        "fn(value: int) -> int { return unknownFunction(value); }",
        "fn() -> () { () }",
        "fn(values: {string: int}) -> {string: int} { values }",
        "fn(callback: fn(int) -> int) -> fn(int) -> int { callback }",
        "fn() -> int { # } remains a comment\n 1 }",
    ] {
        let source = format!("item: Rule {{ apply: {function} }}");
        let (ast, diagnostics) = parse_cfd(&source);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let CfdValue::Function(value) = &ast.records[0].fields[0].value else {
            panic!("function")
        };
        assert_eq!(value.source, function);
        assert_eq!(&source[value.span.start..value.span.end], function);
    }
}

#[test]
fn functions_inside_collections_are_preserved() {
    let (ast, diagnostics) = parse_cfd(
        "item: Rule { callbacks: [fn(x: int) -> int { x }, fn(x: int) -> int { x + 1 }] }",
    );
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let CfdValue::Array(values, _) = &ast.records[0].fields[0].value else {
        panic!("array")
    };
    assert_eq!(values.len(), 2);
    assert!(values
        .iter()
        .all(|value| matches!(value, CfdValue::Function(_))));
}

#[test]
fn ordinary_braces_remain_text_and_f_prefix_preserves_template_source() {
    let (ast, diagnostics) =
        parse_cfd(r#"item: Rule { plain: "{self.name}", text: f"{self.name}" }"#);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert!(
        matches!(&ast.records[0].fields[0].value, CfdValue::QuotedString(text, _) if text == "{self.name}")
    );
    assert!(matches!(
        &ast.records[0].fields[1].value,
        CfdValue::FormattedString(_)
    ));
}

#[test]
fn unterminated_function_body_is_a_source_error() {
    let (_, diagnostics) = parse_cfd("item: Rule { run: fn() -> int { 1");
    assert!(!diagnostics.is_empty());
}
