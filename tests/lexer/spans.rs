use coflow::lexer::{lex, LexErrorKind, TokenKind};
use coflow::span::Span;

#[test]
fn token_spans_are_utf8_byte_offsets() {
    let source = "玩家生命 = \"火球\"\n速度2 ??= 1";
    let output = lex(source);
    assert_eq!(output.errors, []);

    let spans = output
        .tokens
        .iter()
        .map(|token| {
            (
                token.kind,
                token.span,
                &source[token.span.start..token.span.end],
            )
        })
        .collect::<Vec<_>>();

    assert_eq!(
        spans,
        vec![
            (TokenKind::Ident, Span { start: 0, end: 12 }, "玩家生命"),
            (TokenKind::Eq, Span { start: 13, end: 14 }, "="),
            (
                TokenKind::StringLiteral,
                Span { start: 15, end: 23 },
                "\"火球\""
            ),
            (TokenKind::Ident, Span { start: 24, end: 31 }, "速度2"),
            (
                TokenKind::QuestionQuestionEq,
                Span { start: 32, end: 35 },
                "??="
            ),
            (TokenKind::IntLiteral, Span { start: 36, end: 37 }, "1"),
        ]
    );
}

#[test]
fn error_spans_cover_the_unexpected_utf8_character() {
    let source = "ok 😀 next";
    let output = lex(source);

    assert_eq!(output.errors.len(), 1);
    let error = &output.errors[0];
    assert_eq!(error.kind, LexErrorKind::UnexpectedChar);
    assert_eq!(error.span, Span { start: 3, end: 7 });
    assert_eq!(&source[error.span.start..error.span.end], "😀");

    let token_slices = output
        .tokens
        .iter()
        .map(|token| &source[token.span.start..token.span.end])
        .collect::<Vec<_>>();
    assert_eq!(token_slices, vec!["ok", "next"]);
}

#[test]
fn skipped_comments_and_crlf_still_leave_later_spans_correct() {
    let source = "a /* 注释 */\r\nb";
    let output = lex(source);
    assert_eq!(output.errors, []);

    assert_eq!(output.tokens[0].span, Span { start: 0, end: 1 });
    assert_eq!(
        &source[output.tokens[0].span.start..output.tokens[0].span.end],
        "a"
    );
    assert_eq!(output.tokens[1].span, Span { start: 16, end: 17 });
    assert_eq!(
        &source[output.tokens[1].span.start..output.tokens[1].span.end],
        "b"
    );
}
