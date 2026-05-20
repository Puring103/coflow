use coflow::lexer::{lex, LexErrorKind, TokenKind};
use coflow::span::Span;

fn kinds(source: &str) -> Vec<TokenKind> {
    let output = lex(source);
    assert_eq!(output.errors, [], "source: {source}");
    output.tokens.into_iter().map(|token| token.kind).collect()
}

#[test]
fn recognizes_every_keyword_token() {
    assert_eq!(
        kinds(
            "import as local class enum check fn iter dict var if else while until loop for in \
             break continue return throw try catch yield from and or not true false null self"
        ),
        vec![
            TokenKind::Import,
            TokenKind::As,
            TokenKind::Local,
            TokenKind::Class,
            TokenKind::Enum,
            TokenKind::Check,
            TokenKind::Fn,
            TokenKind::Iter,
            TokenKind::Dict,
            TokenKind::Var,
            TokenKind::If,
            TokenKind::Else,
            TokenKind::While,
            TokenKind::Until,
            TokenKind::Loop,
            TokenKind::For,
            TokenKind::In,
            TokenKind::Break,
            TokenKind::Continue,
            TokenKind::Return,
            TokenKind::Throw,
            TokenKind::Try,
            TokenKind::Catch,
            TokenKind::Yield,
            TokenKind::From,
            TokenKind::And,
            TokenKind::Or,
            TokenKind::Not,
            TokenKind::True,
            TokenKind::False,
            TokenKind::Null,
            TokenKind::SelfKw,
        ]
    );
}

#[test]
fn keyword_like_text_stays_identifier() {
    assert_eq!(
        kinds("imported className return_value null_value int float bool string any"),
        vec![
            TokenKind::Ident,
            TokenKind::Ident,
            TokenKind::Ident,
            TokenKind::Ident,
            TokenKind::Ident,
            TokenKind::Ident,
            TokenKind::Ident,
            TokenKind::Ident,
            TokenKind::Ident,
        ]
    );
}

#[test]
fn recognizes_unicode_identifiers() {
    assert_eq!(
        kinds("玩家生命 计算伤害 скорость Δx 名前 速度2 变量_1 _临时值 _1"),
        vec![TokenKind::Ident; 9]
    );
}

#[test]
fn recognizes_every_literal_token() {
    assert_eq!(
        kinds(
            "0 123 1_000 0xff 0XFF 0b1010_0101 0o755 1.0 1_000.5 1.000_5 1e3 1.0e-3 1E+3 \
               \"hero\\n\" r\"C:\\game\" \"\"\"line\nline\"\"\" r\"\"\"C:\\game\nline\"\"\""
        ),
        vec![
            TokenKind::IntLiteral,
            TokenKind::IntLiteral,
            TokenKind::IntLiteral,
            TokenKind::IntLiteral,
            TokenKind::IntLiteral,
            TokenKind::IntLiteral,
            TokenKind::IntLiteral,
            TokenKind::FloatLiteral,
            TokenKind::FloatLiteral,
            TokenKind::FloatLiteral,
            TokenKind::FloatLiteral,
            TokenKind::FloatLiteral,
            TokenKind::FloatLiteral,
            TokenKind::StringLiteral,
            TokenKind::RawStringLiteral,
            TokenKind::MultilineStringLiteral,
            TokenKind::RawMultilineStringLiteral,
        ]
    );
}

#[test]
fn recognizes_every_operator_token() {
    assert_eq!(
        kinds("= += -= *= /= %= ??= **= //= &= |= ^= <<= >>= + - * ** // / % == != < <= << > >= >> ?? . .. ..= ... ?. ?[ -> =>"),
        vec![
            TokenKind::Eq,
            TokenKind::PlusEq,
            TokenKind::MinusEq,
            TokenKind::StarEq,
            TokenKind::SlashEq,
            TokenKind::PercentEq,
            TokenKind::QuestionQuestionEq,
            TokenKind::StarStarEq,
            TokenKind::SlashSlashEq,
            TokenKind::AmpEq,
            TokenKind::PipeEq,
            TokenKind::CaretEq,
            TokenKind::LtLtEq,
            TokenKind::GtGtEq,
            TokenKind::Plus,
            TokenKind::Minus,
            TokenKind::Star,
            TokenKind::StarStar,
            TokenKind::SlashSlash,
            TokenKind::Slash,
            TokenKind::Percent,
            TokenKind::EqEq,
            TokenKind::BangEq,
            TokenKind::Lt,
            TokenKind::LtEq,
            TokenKind::LtLt,
            TokenKind::Gt,
            TokenKind::GtEq,
            TokenKind::GtGt,
            TokenKind::QuestionQuestion,
            TokenKind::Dot,
            TokenKind::DotDot,
            TokenKind::DotDotEq,
            TokenKind::DotDotDot,
            TokenKind::QuestionDot,
            TokenKind::QuestionLBracket,
            TokenKind::Arrow,
            TokenKind::FatArrow,
        ]
    );
}

#[test]
fn recognizes_bitwise_operator_tokens() {
    assert_eq!(
        kinds("& | ^ ~"),
        vec![
            TokenKind::Amp,
            TokenKind::Pipe,
            TokenKind::Caret,
            TokenKind::Tilde,
        ]
    );
}

#[test]
fn recognizes_every_delimiter_token() {
    assert_eq!(
        kinds("( ) { } [ ] , :"),
        vec![
            TokenKind::LParen,
            TokenKind::RParen,
            TokenKind::LBrace,
            TokenKind::RBrace,
            TokenKind::LBracket,
            TokenKind::RBracket,
            TokenKind::Comma,
            TokenKind::Colon,
        ]
    );
}

#[test]
fn negative_numbers_are_minus_plus_literal() {
    assert_eq!(
        kinds("-1 -1.5"),
        vec![
            TokenKind::Minus,
            TokenKind::IntLiteral,
            TokenKind::Minus,
            TokenKind::FloatLiteral,
        ]
    );
}

#[test]
fn floats_do_not_swallow_dot_without_fraction() {
    assert_eq!(
        kinds("1. .5"),
        vec![
            TokenKind::IntLiteral,
            TokenKind::Dot,
            TokenKind::Dot,
            TokenKind::IntLiteral,
        ]
    );
}

#[test]
fn skips_line_comments_with_hash() {
    assert_eq!(
        kinds("a # comment\n b /* hidden */ c"),
        vec![
            TokenKind::Ident,
            TokenKind::Ident,
            TokenKind::Ident,
        ]
    );
}

#[test]
fn slash_slash_is_int_div_not_comment() {
    assert_eq!(
        kinds("a // b"),
        vec![
            TokenKind::Ident,
            TokenKind::SlashSlash,
            TokenKind::Ident,
        ]
    );
}

#[test]
fn spans_are_byte_offsets() {
    let output = lex("玩家生命 = 1");
    assert_eq!(output.errors, []);
    assert_eq!(output.tokens[0].span, Span { start: 0, end: 12 });
    assert_eq!(output.tokens[1].span, Span { start: 13, end: 14 });
}

#[test]
fn every_token_kind_has_a_positive_case() {
    let covered = kinds(
        "import as local class enum check fn iter dict var if else while until loop for in \
         break continue return throw try catch yield from and or not true false null self \
         ident 1 1.0 \"s\" r\"s\" \"\"\"s\"\"\" r\"\"\"s\"\"\" \
         = += -= *= /= %= ??= **= //= &= |= ^= <<= >>= + - * ** // / % == != < <= << > >= >> ?? \
         . .. ..= ... ?. ?[ -> => & | ^ ~ ( ) { } [ ] , :",
    );

    let all = [
        TokenKind::Import,
        TokenKind::As,
        TokenKind::Local,
        TokenKind::Class,
        TokenKind::Enum,
        TokenKind::Check,
        TokenKind::Fn,
        TokenKind::Iter,
        TokenKind::Dict,
        TokenKind::Var,
        TokenKind::If,
        TokenKind::Else,
        TokenKind::While,
        TokenKind::Until,
        TokenKind::Loop,
        TokenKind::For,
        TokenKind::In,
        TokenKind::Break,
        TokenKind::Continue,
        TokenKind::Return,
        TokenKind::Throw,
        TokenKind::Try,
        TokenKind::Catch,
        TokenKind::Yield,
        TokenKind::From,
        TokenKind::And,
        TokenKind::Or,
        TokenKind::Not,
        TokenKind::True,
        TokenKind::False,
        TokenKind::Null,
        TokenKind::SelfKw,
        TokenKind::Ident,
        TokenKind::IntLiteral,
        TokenKind::FloatLiteral,
        TokenKind::StringLiteral,
        TokenKind::RawStringLiteral,
        TokenKind::MultilineStringLiteral,
        TokenKind::RawMultilineStringLiteral,
        TokenKind::Eq,
        TokenKind::PlusEq,
        TokenKind::MinusEq,
        TokenKind::StarEq,
        TokenKind::SlashEq,
        TokenKind::PercentEq,
        TokenKind::QuestionQuestionEq,
        TokenKind::StarStarEq,
        TokenKind::SlashSlashEq,
        TokenKind::AmpEq,
        TokenKind::PipeEq,
        TokenKind::CaretEq,
        TokenKind::LtLtEq,
        TokenKind::GtGtEq,
        TokenKind::Plus,
        TokenKind::Minus,
        TokenKind::Star,
        TokenKind::StarStar,
        TokenKind::SlashSlash,
        TokenKind::Slash,
        TokenKind::Percent,
        TokenKind::EqEq,
        TokenKind::BangEq,
        TokenKind::Lt,
        TokenKind::LtEq,
        TokenKind::LtLt,
        TokenKind::Gt,
        TokenKind::GtEq,
        TokenKind::GtGt,
        TokenKind::QuestionQuestion,
        TokenKind::Dot,
        TokenKind::DotDot,
        TokenKind::DotDotEq,
        TokenKind::DotDotDot,
        TokenKind::QuestionDot,
        TokenKind::QuestionLBracket,
        TokenKind::Arrow,
        TokenKind::FatArrow,
        TokenKind::Amp,
        TokenKind::Pipe,
        TokenKind::Caret,
        TokenKind::Tilde,
        TokenKind::LParen,
        TokenKind::RParen,
        TokenKind::LBrace,
        TokenKind::RBrace,
        TokenKind::LBracket,
        TokenKind::RBracket,
        TokenKind::Comma,
        TokenKind::Colon,
    ];

    for kind in all {
        assert!(
            covered.contains(&kind),
            "missing positive case for {kind:?}"
        );
    }
}

#[test]
fn every_error_kind_has_a_negative_case() {
    let cases = [
        ("?", LexErrorKind::UnexpectedChar),
        ("\"unterminated", LexErrorKind::UnterminatedString),
        ("/* unterminated", LexErrorKind::UnterminatedBlockComment),
        ("\"bad \\x\"", LexErrorKind::InvalidEscape),
        ("1_", LexErrorKind::InvalidNumber),
    ];

    for (source, expected) in cases {
        let output = lex(source);
        assert_eq!(
            output.errors.first().map(|error| error.kind),
            Some(expected),
            "source: {source}"
        );
    }
}
