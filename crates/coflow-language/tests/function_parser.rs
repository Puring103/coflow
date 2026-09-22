use coflow_language::{
    function::{
        parse_expression, parse_function, parse_function_with_limits, ExprKind, StatementKind,
        TemplatePart,
    },
    limits::StructuralLimits,
};

#[test]
fn arithmetic_obeys_power_associativity_and_unary_precedence() {
    let expression = parse_expression("-2 ** 3 ** 2 + 5 * 6").expect("expression");
    let ExprKind::Binary {
        operator,
        left,
        right,
    } = expression.kind
    else {
        panic!("sum")
    };
    assert_eq!(operator, "+");
    assert!(matches!(right.kind, ExprKind::Binary { operator, .. } if operator == "*"));
    let ExprKind::Binary {
        operator,
        left,
        right,
    } = left.kind
    else {
        panic!("power")
    };
    assert_eq!(operator, "**");
    assert!(matches!(left.kind, ExprKind::Unary { operator, .. } if operator == "-"));
    assert!(matches!(right.kind, ExprKind::Binary { operator, .. } if operator == "**"));
}

#[test]
fn loops_assignments_and_nested_closures_share_function_syntax() {
    let source = r#"fn(values: [int], limit: int) -> fn() -> int {
        var total: int = 0;
        for index, value in values {
            if index >= limit { break; }
            if value < 0 { continue; }
            total += value;
        }
        while total < limit { total <<= 1; }
        for value in 0..=limit { total += value; }
        fn() -> int { total }
    }"#;
    let function = parse_function(source).expect("function");
    assert_eq!(function.parameters.len(), 2);
    assert_eq!(function.body.statements.len(), 4);
    assert!(
        matches!(&function.body.statements[1].kind, StatementKind::For { bindings, .. } if bindings == &["index", "value"])
    );
    assert!(matches!(
        &function.body.tail.expect("tail").kind,
        ExprKind::Function(_)
    ));
    assert_eq!(&source[function.span.start..function.span.end], source);
}

#[test]
fn data_and_record_expressions_keep_distinct_shapes() {
    for source in [
        "Item { name: \"名字\", owner: &Character::hero, values: {\"z\": 1, \"a\": 2} }",
        "[fn() -> int { 1 }, fn() -> int { 2 }]",
        "if optional is Some(value) && value > 0 { value } else { 0 }",
        "self.children[0].read(1, 2,)?",
        "if (Item { name: \"A\" }).ready { 1 } else if true { 2 } else { 3 }",
        "fn(callback: fn(int) -> int, table: {string: int}?) -> () { () }",
    ] {
        // table 是保留字，参数位置必须拒绝；其余片段覆盖表达式中的复合形状。
        if source.contains("table:") {
            assert!(parse_expression(source).is_err());
        } else {
            parse_expression(source).unwrap_or_else(|error| panic!("{source}: {error}"));
        }
    }
    let expression = parse_expression("&Game::Item::entry").expect("reference");
    assert!(
        matches!(expression.kind, ExprKind::Reference { type_name: Some(name), key } if name == "Game::Item" && key == "entry")
    );
}

#[test]
fn template_interpolations_use_original_utf8_offsets_and_nested_syntax() {
    let source = r#"f"名字 {{ok}} {self.name} {if true { "值" } else { "空" }}\n""#;
    let expression = parse_expression(source).expect("template");
    let ExprKind::Template(parts) = expression.kind else {
        panic!("template")
    };
    assert_eq!(parts[0], TemplatePart::Text("名字 {ok} ".into()));
    let expressions: Vec<_> = parts
        .iter()
        .filter_map(|part| match part {
            TemplatePart::Expression(expression) => Some(expression),
            _ => None,
        })
        .collect();
    assert_eq!(expressions.len(), 2);
    assert_eq!(
        &source[expressions[0].span.start..expressions[0].span.end],
        "self.name"
    );
    assert_eq!(
        &source[expressions[1].span.start..expressions[1].span.end],
        "if true { \"值\" } else { \"空\" }"
    );
    assert_eq!(parts.last(), Some(&TemplatePart::Text("\n".into())));
}

#[test]
fn functions_reject_missing_types_delimiters_and_trailing_input() {
    for source in [
        "fn(value) -> int { value }",
        "fn() -> int { var value = 1; value }",
        "fn() -> int { var self: int = 1; self }",
        "fn() -> int { 1 2 }",
        "fn() -> int { 1 } trailing",
        "fn() -> int { (1 + 2 }",
        "fn() -> int { for a, b, c in values {} 1 }",
        "fn() -> int { match x { _ => 1 } }",
    ] {
        assert!(parse_function(source).is_err(), "{source}");
    }
    for source in ["a < b < c", "value??", "1e+", "\"unterminated", "{1, 2}"] {
        assert!(parse_expression(source).is_err(), "{source}");
    }
}

#[test]
fn comments_cannot_hide_operators_or_terminate_function_bodies() {
    let source = "fn() -> int { # }\r\n var 值: int = 4; 值 //= 2; 值 }";
    let parsed = parse_function(source).expect("function");
    let StatementKind::Assign { name, operator, .. } = &parsed.body.statements[1].kind else {
        panic!("assignment")
    };
    assert_eq!((name.as_str(), operator.as_str()), ("值", "//="));
    assert_eq!(
        &source[parsed.body.statements[1].span.start..parsed.body.statements[1].span.end],
        "值 //= 2;"
    );
}

#[test]
fn structural_limits_cover_recursive_syntax_left_chains_and_else_if() {
    let limits = StructuralLimits::new(16, 1_000, 10_000);
    for expression in [
        format!("{}1{}", "(".repeat(100), ")".repeat(100)),
        format!("{}1", "1 + ".repeat(100)),
        format!("{}{{ 0 }}", "if true { 1 } else ".repeat(100)),
    ] {
        let source = format!("fn() -> int {{ {expression} }}");
        let error = parse_function_with_limits(&source, limits).expect_err("limit");
        assert!(error.message.contains("超限"), "{error}");
    }
    assert!(
        parse_function_with_limits("fn() -> int { 1 }", StructuralLimits::new(100, 1, 100))
            .is_err()
    );
    assert!(
        parse_function_with_limits("fn() -> int { 1 }", StructuralLimits::new(100, 100, 1))
            .is_err()
    );
}

#[test]
fn nested_syntax_uses_a_bounded_native_stack() {
    std::thread::Builder::new().stack_size(1024 * 1024).spawn(|| {
        for (open, close, depth) in [
            ("-", "", 200), ("1+(", ")", 100), ("[", "]", 200),
            ("A { x:", "}", 200), ("f(", ")", 200), ("a[", "]", 200),
            ("if true {", "} else {0}", 80), ("fn()->int {", "}", 80),
            ("f\"{", "}\"", 80),
        ] {
            let source = format!("{}1{}", open.repeat(depth), close.repeat(depth));
            assert!(parse_expression(&source).is_ok(), "{open}");
            let source = format!("{}1{}", open.repeat(300), close.repeat(300));
            let error = parse_expression(&source).expect_err("结构上限必须在宿主栈耗尽前生效");
            assert!(error.message.contains("超限"), "{open}: {error}");
        }
        let source = format!("fn(value: {}int{}) -> int {{ 1 }}", "[".repeat(200), "]".repeat(200));
        assert!(parse_function(&source).is_ok());
        let source = format!("fn()->int {{ {}1{} }}", "while true {".repeat(200), "}".repeat(200));
        assert!(parse_function(&source).is_ok());
    }).unwrap().join().unwrap();
}
