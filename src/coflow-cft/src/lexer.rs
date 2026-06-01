use crate::error::{ParseErrorKind, ParseErrors};
use crate::span::Span;
use unicode_ident::{is_xid_continue, is_xid_start};

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    Ident(String),
    Int(i64),
    Float(f64),
    String(String),
    Type,
    Enum,
    Check,
    Assert,
    All,
    Any,
    None,
    Null,
    In,
    Is,
    True,
    False,
    Dict,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    LParen,
    RParen,
    Colon,
    Semicolon,
    Comma,
    Dot,
    Equal,
    Plus,
    Minus,
    Star,
    Slash,
    SlashSlash,
    Percent,
    StarStar,
    Less,
    Greater,
    Bang,
    Question,
    Tilde,
    Amp,
    Pipe,
    Caret,
    AmpAmp,
    PipePipe,
    LessEq,
    GreaterEq,
    LessLess,
    GreaterGreater,
    EqEq,
    BangEq,
    Eof,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

#[allow(clippy::too_many_lines)]
pub fn lex(source: &str) -> Result<Vec<Token>, ParseErrors> {
    let bytes = source.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0;
    let mut check_depth = 0usize;
    let mut pending_check_brace = false;
    while i < bytes.len() {
        let Some(ch) = source[i..].chars().next() else {
            break;
        };
        let ch_len = ch.len_utf8();
        if ch.is_ascii_whitespace() {
            i += ch_len;
            continue;
        }
        if bytes[i] == b'/'
            && bytes.get(i + 1) == Some(&b'/')
            && (check_depth == 0 || !slash_slash_is_operator(source, i))
        {
            i += 2;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }

        let start = i;
        let kind = match ch {
            '{' => {
                i += ch_len;
                TokenKind::LBrace
            }
            '}' => {
                i += ch_len;
                TokenKind::RBrace
            }
            '[' => {
                i += ch_len;
                TokenKind::LBracket
            }
            ']' => {
                i += ch_len;
                TokenKind::RBracket
            }
            '(' => {
                i += ch_len;
                TokenKind::LParen
            }
            ')' => {
                i += ch_len;
                TokenKind::RParen
            }
            ':' => {
                i += ch_len;
                TokenKind::Colon
            }
            ';' => {
                i += ch_len;
                TokenKind::Semicolon
            }
            ',' => {
                i += ch_len;
                TokenKind::Comma
            }
            '.' => {
                i += ch_len;
                TokenKind::Dot
            }
            '+' => {
                i += ch_len;
                TokenKind::Plus
            }
            '-' => {
                i += ch_len;
                TokenKind::Minus
            }
            '*' if bytes.get(i + 1) == Some(&b'*') => {
                i += 2;
                TokenKind::StarStar
            }
            '*' => {
                i += ch_len;
                TokenKind::Star
            }
            '/' if bytes.get(i + 1) == Some(&b'/') => {
                i += 2;
                TokenKind::SlashSlash
            }
            '/' => {
                i += ch_len;
                TokenKind::Slash
            }
            '%' => {
                i += ch_len;
                TokenKind::Percent
            }
            '=' if bytes.get(i + 1) == Some(&b'=') => {
                i += 2;
                TokenKind::EqEq
            }
            '=' => {
                i += ch_len;
                TokenKind::Equal
            }
            '<' if bytes.get(i + 1) == Some(&b'=') => {
                i += 2;
                TokenKind::LessEq
            }
            '<' if bytes.get(i + 1) == Some(&b'<') => {
                i += 2;
                TokenKind::LessLess
            }
            '<' => {
                i += ch_len;
                TokenKind::Less
            }
            '>' if bytes.get(i + 1) == Some(&b'=') => {
                i += 2;
                TokenKind::GreaterEq
            }
            '>' if bytes.get(i + 1) == Some(&b'>') => {
                i += 2;
                TokenKind::GreaterGreater
            }
            '>' => {
                i += ch_len;
                TokenKind::Greater
            }
            '!' if bytes.get(i + 1) == Some(&b'=') => {
                i += 2;
                TokenKind::BangEq
            }
            '!' => {
                i += ch_len;
                TokenKind::Bang
            }
            '?' => {
                i += ch_len;
                TokenKind::Question
            }
            '&' if bytes.get(i + 1) == Some(&b'&') => {
                i += 2;
                TokenKind::AmpAmp
            }
            '&' => {
                i += ch_len;
                TokenKind::Amp
            }
            '|' if bytes.get(i + 1) == Some(&b'|') => {
                i += 2;
                TokenKind::PipePipe
            }
            '|' => {
                i += ch_len;
                TokenKind::Pipe
            }
            '^' => {
                i += ch_len;
                TokenKind::Caret
            }
            '~' => {
                i += ch_len;
                TokenKind::Tilde
            }
            '"' => lex_string(source, &mut i, start)?,
            '0'..='9' => lex_number(source, &mut i, start)?,
            '_' => lex_ident(source, &mut i),
            c if is_xid_start(c) => lex_ident(source, &mut i),
            _ => {
                return Err(ParseErrors::one_kind(
                    ParseErrorKind::Lex,
                    format!("unexpected character `{ch}`"),
                    Span::new(start, start + ch_len),
                ));
            }
        };
        update_check_depth(&kind, &mut check_depth, &mut pending_check_brace);
        tokens.push(Token {
            kind,
            span: Span::new(start, i),
        });
    }
    tokens.push(Token {
        kind: TokenKind::Eof,
        span: Span::new(source.len(), source.len()),
    });
    Ok(tokens)
}

fn lex_ident(source: &str, i: &mut usize) -> TokenKind {
    let start = *i;
    bump_char(source, i);
    while let Some(ch) = source[*i..].chars().next() {
        if ch == '_' || is_xid_continue(ch) {
            *i += ch.len_utf8();
        } else {
            break;
        }
    }
    let text = &source[start..*i];
    match text {
        "type" => TokenKind::Type,
        "enum" => TokenKind::Enum,
        "check" => TokenKind::Check,
        "assert" => TokenKind::Assert,
        "all" => TokenKind::All,
        "any" => TokenKind::Any,
        "none" => TokenKind::None,
        "null" => TokenKind::Null,
        "in" => TokenKind::In,
        "is" => TokenKind::Is,
        "true" => TokenKind::True,
        "false" => TokenKind::False,
        "dict" => TokenKind::Dict,
        _ => TokenKind::Ident(text.to_string()),
    }
}

fn lex_number(source: &str, i: &mut usize, start: usize) -> Result<TokenKind, ParseErrors> {
    let bytes = source.as_bytes();
    while *i < bytes.len() && bytes[*i].is_ascii_digit() {
        *i += 1;
    }
    let mut is_float = false;
    if bytes.get(*i) == Some(&b'.') && bytes.get(*i + 1).is_some_and(u8::is_ascii_digit) {
        is_float = true;
        *i += 1;
        while *i < bytes.len() && bytes[*i].is_ascii_digit() {
            *i += 1;
        }
    }
    let raw = &source[start..*i];
    if is_float {
        raw.parse::<f64>().map(TokenKind::Float).map_err(|_| {
            ParseErrors::one_kind(
                ParseErrorKind::Lex,
                "invalid float literal",
                Span::new(start, *i),
            )
        })
    } else {
        raw.parse::<i64>().map(TokenKind::Int).map_err(|_| {
            ParseErrors::one_kind(
                ParseErrorKind::Lex,
                "invalid int literal",
                Span::new(start, *i),
            )
        })
    }
}

fn lex_string(source: &str, i: &mut usize, start: usize) -> Result<TokenKind, ParseErrors> {
    let bytes = source.as_bytes();
    *i += 1;
    let mut out = String::new();
    while *i < bytes.len() {
        match source[*i..].chars().next() {
            Some('"') => {
                *i += 1;
                return Ok(TokenKind::String(out));
            }
            Some('\\') => {
                *i += 1;
                let Some(escaped) = bytes.get(*i).copied() else {
                    break;
                };
                let ch = match escaped {
                    b'"' => '"',
                    b'\\' => '\\',
                    b'n' => '\n',
                    b'r' => '\r',
                    b't' => '\t',
                    _ => {
                        return Err(ParseErrors::one_kind(
                            ParseErrorKind::Lex,
                            "invalid string escape",
                            Span::new(*i - 1, *i + 1),
                        ));
                    }
                };
                out.push(ch);
                *i += 1;
            }
            Some(ch) => {
                out.push(ch);
                *i += ch.len_utf8();
            }
            None => break,
        }
    }
    Err(ParseErrors::one_kind(
        ParseErrorKind::Lex,
        "unterminated string literal",
        Span::new(start, source.len()),
    ))
}

fn bump_char(source: &str, i: &mut usize) {
    if let Some(ch) = source[*i..].chars().next() {
        *i += ch.len_utf8();
    }
}

fn update_check_depth(kind: &TokenKind, check_depth: &mut usize, pending_check_brace: &mut bool) {
    match kind {
        TokenKind::Check => {
            *pending_check_brace = true;
        }
        TokenKind::LBrace if *pending_check_brace => {
            *pending_check_brace = false;
            *check_depth += 1;
        }
        TokenKind::LBrace if *check_depth > 0 => {
            *check_depth += 1;
        }
        TokenKind::RBrace if *check_depth > 0 => {
            *check_depth -= 1;
        }
        _ if !matches!(kind, TokenKind::LBrace) => {
            *pending_check_brace = false;
        }
        _ => {}
    }
}

fn slash_slash_is_operator(source: &str, pos: usize) -> bool {
    let Some(prev) = prev_non_ws(source, pos) else {
        return false;
    };
    let Some(next) = next_non_ws(source, pos + 2) else {
        return false;
    };
    let prev_can_end_operand =
        prev.is_ascii_alphanumeric() || prev == '_' || matches!(prev, ')' | ']' | '"');
    let next_can_start_operand =
        next.is_ascii_alphanumeric() || next == '_' || matches!(next, '(' | '"' | '-' | '!' | '~');
    prev_can_end_operand && next_can_start_operand
}

fn prev_non_ws(source: &str, pos: usize) -> Option<char> {
    source[..pos].chars().rev().find(|ch| !ch.is_whitespace())
}

fn next_non_ws(source: &str, pos: usize) -> Option<char> {
    source[pos..].chars().find(|ch| !ch.is_whitespace())
}
