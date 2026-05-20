use coflow::lexer::{lex, LexErrorKind, TokenKind};

#[test]
fn reports_multiple_errors_and_keeps_following_valid_tokens() {
    let source = "? ok @ done";
    let output = lex(source);

    assert_eq!(
        output
            .errors
            .iter()
            .map(|error| error.kind)
            .collect::<Vec<_>>(),
        vec![LexErrorKind::UnexpectedChar, LexErrorKind::UnexpectedChar]
    );
    assert_eq!(
        output
            .tokens
            .iter()
            .map(|token| token.kind)
            .collect::<Vec<_>>(),
        vec![TokenKind::Ident, TokenKind::Ident]
    );
}

#[test]
fn invalid_number_consumes_the_identifier_suffix_before_recovering() {
    let source = "123abc next";
    let output = lex(source);

    assert_eq!(output.errors.len(), 1);
    assert_eq!(output.errors[0].kind, LexErrorKind::InvalidNumber);
    assert_eq!(
        &source[output.errors[0].span.start..output.errors[0].span.end],
        "123abc"
    );
    assert_eq!(
        output
            .tokens
            .iter()
            .map(|token| token.kind)
            .collect::<Vec<_>>(),
        vec![TokenKind::Ident]
    );
    assert_eq!(
        &source[output.tokens[0].span.start..output.tokens[0].span.end],
        "next"
    );
}
