use crate::syntax::lexer::TokenKind;

pub(super) fn token_name(kind: &TokenKind) -> &'static str {
    match kind {
        TokenKind::LBrace => "{",
        TokenKind::RBrace => "}",
        TokenKind::LBracket => "[",
        TokenKind::RBracket => "]",
        TokenKind::LParen => "(",
        TokenKind::RParen => ")",
        TokenKind::Colon => ":",
        TokenKind::Semicolon => ";",
        TokenKind::Comma => ",",
        TokenKind::Dot => ".",
        TokenKind::Equal => "=",
        TokenKind::Question => "?",
        TokenKind::QuestionQuestion => "??",
        TokenKind::FormattedStringStart => "formatted string",
        TokenKind::FormattedStringExprStart => "{",
        TokenKind::FormattedStringExprEnd => "}",
        TokenKind::FormattedStringEnd => "\"",
        TokenKind::In => "in",
        _ => "token",
    }
}

pub(super) fn reserved_keyword_name(kind: &TokenKind) -> Option<&'static str> {
    match kind {
        TokenKind::Const => Some("const"),
        TokenKind::Enum => Some("enum"),
        TokenKind::Type => Some("type"),
        TokenKind::Abstract => Some("abstract"),
        TokenKind::Sealed => Some("sealed"),
        TokenKind::Check => Some("check"),
        TokenKind::When => Some("when"),
        TokenKind::All => Some("all"),
        TokenKind::Any => Some("any"),
        TokenKind::None => Some("none"),
        TokenKind::In => Some("in"),
        TokenKind::Is => Some("is"),
        TokenKind::True => Some("true"),
        TokenKind::False => Some("false"),
        TokenKind::Null => Some("null"),
        _ => None,
    }
}
