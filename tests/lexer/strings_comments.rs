use coflow::lexer::{lex, LexErrorKind, TokenKind};

fn kinds(source: &str) -> Vec<TokenKind> {
    let output = lex(source);
    assert_eq!(output.errors, [], "source: {source}");
    output.tokens.into_iter().map(|token| token.kind).collect()
}

fn first_error(source: &str) -> LexErrorKind {
    lex(source)
        .errors
        .first()
        .unwrap_or_else(|| panic!("expected lex error for {source:?}"))
        .kind
}

#[test]
fn accepts_all_string_literal_forms() {
    let cases = [
        ("\"plain\"", TokenKind::StringLiteral),
        (
            "\"quote: \\\" slash: \\\\ tab: \\t\"",
            TokenKind::StringLiteral,
        ),
        (r#"r"C:\game\hero\n""#, TokenKind::RawStringLiteral),
        ("\"\"\"line\nline\"\"\"", TokenKind::MultilineStringLiteral),
        (
            "r\"\"\"C:\\game\\hero\nline\"\"\"",
            TokenKind::RawMultilineStringLiteral,
        ),
    ];

    for (source, expected) in cases {
        assert_eq!(kinds(source), vec![expected], "source: {source}");
    }
}

#[test]
fn raw_strings_allow_escape_looking_text() {
    assert_eq!(
        kinds(r#"r"\x \u \0 \n""#),
        vec![TokenKind::RawStringLiteral]
    );
    assert_eq!(
        kinds("r\"\"\"\\x\n\\u\n\\0\"\"\""),
        vec![TokenKind::RawMultilineStringLiteral]
    );
}

#[test]
fn non_raw_strings_reject_unsupported_escape_sequences() {
    for source in ["\"bad \\x\"", "\"bad \\u\"", "\"bad \\0\""] {
        assert_eq!(
            first_error(source),
            LexErrorKind::InvalidEscape,
            "source: {source}"
        );
    }
}

#[test]
fn single_line_strings_cannot_cross_newlines() {
    for source in ["\"line\nnext\"", "r\"line\nnext\""] {
        assert_eq!(
            first_error(source),
            LexErrorKind::UnterminatedString,
            "source: {source}"
        );
    }
}

#[test]
fn comments_are_skipped_without_creating_tokens() {
    assert_eq!(
        kinds("a # hidden + - *\r\n b /* hidden { [ ( */ c"),
        vec![TokenKind::Ident, TokenKind::Ident, TokenKind::Ident]
    );
}

#[test]
fn unterminated_block_comments_report_from_comment_start() {
    let source = "value = 1 /* comment";
    let output = lex(source);
    assert_eq!(output.errors.len(), 1);
    assert_eq!(
        output.errors[0].kind,
        LexErrorKind::UnterminatedBlockComment
    );
    assert_eq!(&source[output.errors[0].span.start..], "/* comment");
}
