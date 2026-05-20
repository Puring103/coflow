use coflow::lexer::{lex, LexErrorKind, TokenKind};

fn single_kind(source: &str) -> TokenKind {
    let output = lex(source);
    assert_eq!(output.errors, [], "source: {source}");
    assert_eq!(output.tokens.len(), 1, "source: {source}");
    output.tokens[0].kind
}

fn first_error(source: &str) -> LexErrorKind {
    let output = lex(source);
    output
        .errors
        .first()
        .unwrap_or_else(|| panic!("expected lex error for {source:?}"))
        .kind
}

#[test]
fn accepts_decimal_integer_forms() {
    for source in ["0", "7", "123", "1_000", "12_345_678"] {
        assert_eq!(
            single_kind(source),
            TokenKind::IntLiteral,
            "source: {source}"
        );
    }
}

#[test]
fn accepts_prefixed_integer_forms() {
    for source in [
        "0xff",
        "0XFF",
        "0xCAFE_BABE",
        "0b1010",
        "0B1010_0101",
        "0o755",
        "0O7_5_5",
    ] {
        assert_eq!(
            single_kind(source),
            TokenKind::IntLiteral,
            "source: {source}"
        );
    }
}

#[test]
fn accepts_decimal_float_and_scientific_forms() {
    for source in [
        "1.0",
        "1_000.5",
        "1.000_5",
        "0.0",
        "1e3",
        "1E3",
        "1e+3",
        "1E-3",
        "1.0e-3",
        "1_000.5e+10",
    ] {
        assert_eq!(
            single_kind(source),
            TokenKind::FloatLiteral,
            "source: {source}"
        );
    }
}

#[test]
fn dot_boundary_cases_do_not_create_implicit_floats() {
    let output = lex("1. .5 1..2");
    assert_eq!(output.errors, []);
    assert_eq!(
        output
            .tokens
            .iter()
            .map(|token| token.kind)
            .collect::<Vec<_>>(),
        vec![
            TokenKind::IntLiteral,
            TokenKind::Dot,
            TokenKind::Dot,
            TokenKind::IntLiteral,
            TokenKind::IntLiteral,
            TokenKind::Dot,
            TokenKind::Dot,
            TokenKind::IntLiteral,
        ]
    );
}

#[test]
fn rejects_invalid_separator_placement() {
    for source in [
        "1_", "1__0", "1_.0", "1._0", "1.0_", "1__0.0", "1.0__0", "1_e3", "1e_3", "1e+_3",
    ] {
        assert_eq!(
            first_error(source),
            LexErrorKind::InvalidNumber,
            "source: {source}"
        );
    }
}

#[test]
fn rejects_invalid_prefixed_numbers() {
    for source in [
        "0x", "0X", "0x_1", "0x1_", "0x1__2", "0xzz", "0b", "0b_1", "0b102", "0o", "0o_7", "0o789",
        "0o7_",
    ] {
        assert_eq!(
            first_error(source),
            LexErrorKind::InvalidNumber,
            "source: {source}"
        );
    }
}

#[test]
fn rejects_numbers_followed_by_identifier_text() {
    for source in ["123abc", "123_abc", "123玩家", "1e3ms", "0x1变量"] {
        assert_eq!(
            first_error(source),
            LexErrorKind::InvalidNumber,
            "source: {source}"
        );
    }
}

#[test]
fn rejects_scientific_notation_without_exponent_digits() {
    for source in ["1e", "1E", "1e+", "1e-", "1.0e", "1.0e+"] {
        assert_eq!(
            first_error(source),
            LexErrorKind::InvalidNumber,
            "source: {source}"
        );
    }
}
