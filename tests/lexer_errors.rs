use coflow::lexer::{lex, LexErrorKind};

fn first_error(source: &str) -> LexErrorKind {
    lex(source).errors.first().expect("expected lex error").kind
}

#[test]
fn reports_unexpected_characters() {
    for source in ["?", "!", "|", "&", "^", "player? .name"] {
        assert_eq!(
            first_error(source),
            LexErrorKind::UnexpectedChar,
            "source: {source}"
        );
    }
}

#[test]
fn reports_multiple_unexpected_characters_and_recovers() {
    let output = lex("? | !");
    assert_eq!(
        output
            .errors
            .iter()
            .map(|error| error.kind)
            .collect::<Vec<_>>(),
        vec![
            LexErrorKind::UnexpectedChar,
            LexErrorKind::UnexpectedChar,
            LexErrorKind::UnexpectedChar
        ]
    );
}

#[test]
fn reports_invalid_integer_numbers() {
    for source in [
        "1_", "1__0", "0_x", "0x_ff", "0b_1010", "0o755_", "0b102", "0o789", "0xzz", "0x", "0b",
        "0o", "123abc", "1变量",
    ] {
        assert_eq!(
            first_error(source),
            LexErrorKind::InvalidNumber,
            "source: {source}"
        );
    }
}

#[test]
fn reports_invalid_float_numbers() {
    for source in ["1_.0", "1._0", "1.0_", "1__0.0", "1.0__0"] {
        assert_eq!(
            first_error(source),
            LexErrorKind::InvalidNumber,
            "source: {source}"
        );
    }
}

#[test]
fn reports_unsupported_scientific_numbers() {
    for source in ["1e3", "1.0e-3", "1E3"] {
        assert_eq!(
            first_error(source),
            LexErrorKind::InvalidNumber,
            "source: {source}"
        );
    }
}

#[test]
fn reports_string_errors() {
    for source in ["\"bad \\x\"", "\"bad \\u\"", "\"bad \\0\""] {
        assert_eq!(
            first_error(source),
            LexErrorKind::InvalidEscape,
            "source: {source}"
        );
    }

    for source in [
        "\"unterminated",
        "\"line\nnext\"",
        "r\"unterminated",
        "\"\"\"unterminated",
        "r\"\"\"unterminated",
    ] {
        assert_eq!(
            first_error(source),
            LexErrorKind::UnterminatedString,
            "source: {source}"
        );
    }
}

#[test]
fn reports_comment_errors() {
    assert_eq!(
        first_error("value = 1 /* comment"),
        LexErrorKind::UnterminatedBlockComment
    );
}

#[test]
fn reports_invalid_unicode_identifier_characters() {
    assert_eq!(first_error("hp😀value"), LexErrorKind::UnexpectedChar);
    assert_eq!(
        first_error("value\u{200B}name"),
        LexErrorKind::UnexpectedChar
    );
}
