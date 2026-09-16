use coflow_language::lexical::{tokenize_lossless, LosslessTokenKind};

#[test]
fn every_assignment_operator_is_one_token_with_exact_utf8_span() {
    for operator in [
        "=", "+=", "-=", "*=", "/=", "%=", "//=", "**=", "<<=", ">>=", "&=", "|=", "^=",
    ] {
        let source = format!("变量{operator}-2; # 运算符 {operator}\r\n");
        let tokens = tokenize_lossless(&source);
        let significant: Vec<_> = tokens.iter().filter(|token| !token.is_trivia()).collect();
        assert_eq!(
            significant
                .iter()
                .map(|token| token.text(&source))
                .collect::<Vec<_>>(),
            ["变量", operator, "-", "2", ";"],
            "{source}"
        );
        let token = significant[1];
        assert_eq!(token.kind, LosslessTokenKind::Symbol);
        assert_eq!(token.span.start, "变量".len());
        assert_eq!(token.span.end, "变量".len() + operator.len());
        assert_eq!(
            tokens
                .iter()
                .map(|token| token.text(&source))
                .collect::<String>(),
            source
        );
    }
}

#[test]
fn whitespace_and_comments_do_not_join_operator_tokens() {
    for source in [
        "value // = 2",
        "value ** = 2",
        "value << = 2",
        "value >> = 2",
        "value % # note\n= 2",
    ] {
        let text: Vec<_> = tokenize_lossless(source)
            .into_iter()
            .filter(|token| !token.is_trivia())
            .map(|token| token.text(source))
            .collect();
        assert_eq!(text.len(), 4, "{source}: {text:?}");
        assert_eq!(text[2], "=", "{source}");
    }
}

#[test]
fn integer_division_and_ranges_keep_numeric_boundaries() {
    for (source, expected) in [
        ("1//2", vec!["1", "//", "2"]),
        ("1..2", vec!["1", "..", "2"]),
        ("1..=2", vec!["1", "..=", "2"]),
        ("1.25..=2.5", vec!["1.25", "..=", "2.5"]),
    ] {
        let text: Vec<_> = tokenize_lossless(source)
            .into_iter()
            .filter(|token| !token.is_trivia())
            .map(|token| token.text(source))
            .collect();
        assert_eq!(text, expected, "{source}");
    }
}
