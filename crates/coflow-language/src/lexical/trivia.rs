use crate::source::Span;

/// 无损词法单元类别；源码文本始终通过 `span` 从原始输入切取。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LosslessTokenKind {
    Whitespace,
    Newline,
    Comment,
    Identifier,
    Number,
    String,
    Symbol,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LosslessToken {
    pub kind: LosslessTokenKind,
    pub span: Span,
}

impl LosslessToken {
    #[must_use]
    pub fn text<'a>(&self, source: &'a str) -> &'a str {
        &source[self.span.start..self.span.end]
    }

    #[must_use]
    pub fn is_trivia(self) -> bool {
        matches!(
            self.kind,
            LosslessTokenKind::Whitespace | LosslessTokenKind::Newline | LosslessTokenKind::Comment
        )
    }
}

const SYMBOLS: &[&str] = &[
    "..=", "->", "=>", "::", "//", "**", "<<", ">>", "<=", ">=", "==", "!=", "&&", "||",
    "+=", "-=", "*=", "/=", "..", "(", ")", "{", "}", "[", "]", ",", ":", ";", ".", "?",
    "+", "-", "*", "/", "%", "~", "!", "&", "|", "^", "<", ">", "=", "@",
];

/// 扫描完整源码并保留所有 trivia 与未知字符，任意输入都能无损重建。
#[must_use]
pub fn tokenize_lossless(source: &str) -> Vec<LosslessToken> {
    let mut tokens = Vec::new();
    let mut offset = 0;
    while offset < source.len() {
        let start = offset;
        let ch = next_char(source, offset);
        let kind = if matches!(ch, '\r' | '\n') {
            offset += ch.len_utf8();
            if ch == '\r' && source[offset..].starts_with('\n') {
                offset += 1;
            }
            LosslessTokenKind::Newline
        } else if ch.is_whitespace() {
            offset = take_while(source, offset + ch.len_utf8(), |next| {
                next.is_whitespace() && !matches!(next, '\r' | '\n')
            });
            LosslessTokenKind::Whitespace
        } else if ch == '#' {
            offset = take_while(source, offset + 1, |next| !matches!(next, '\r' | '\n'));
            LosslessTokenKind::Comment
        } else if ch == '"' {
            offset = scan_string(source, offset);
            LosslessTokenKind::String
        } else if ch == '$'
            && source[offset + 1..]
                .chars()
                .next()
                .is_some_and(super::is_identifier_start)
        {
            offset = scan_identifier(source, offset + 1);
            LosslessTokenKind::Identifier
        } else if super::is_identifier_start(ch) {
            offset = scan_identifier(source, offset);
            LosslessTokenKind::Identifier
        } else if ch.is_ascii_digit() {
            offset = scan_number(source, offset);
            LosslessTokenKind::Number
        } else if let Some(symbol) = SYMBOLS
            .iter()
            .find(|symbol| source[offset..].starts_with(**symbol))
        {
            offset += symbol.len();
            LosslessTokenKind::Symbol
        } else {
            offset += ch.len_utf8();
            LosslessTokenKind::Unknown
        };
        tokens.push(LosslessToken {
            kind,
            span: Span::new(start, offset),
        });
    }
    tokens
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LiteralError {
    pub(crate) message: String,
    pub(crate) offset: usize,
}

pub(crate) fn validate_number_literal(source: &str) -> Result<(), LiteralError> {
    let Some(exponent) = source.find(['e', 'E']) else {
        return Ok(());
    };
    let digits = source[exponent + 1..].trim_start_matches(['+', '-']);
    if digits.is_empty() {
        Err(LiteralError {
            message: "expected exponent digits".into(),
            offset: source.len(),
        })
    } else {
        Ok(())
    }
}

/// 校验函数语言使用的格式化字符串转义与插值括号。
pub(crate) fn validate_formatted_string_literal(source: &str) -> Result<(), LiteralError> {
    let mut offset = 1;
    let mut interpolation = 0usize;
    let mut closed = false;
    while offset < source.len() {
        let ch = next_char(source, offset);
        offset += ch.len_utf8();
        match ch {
            '\\' => {
                let escaped = (offset < source.len()).then(|| next_char(source, offset));
                let Some(escaped) = escaped else {
                    return Err(LiteralError {
                        message: "unterminated string escape".into(),
                        offset: 0,
                    });
                };
                if !matches!(escaped, '"' | '\\' | 'n' | 'r' | 't') {
                    return Err(LiteralError {
                        message: format!("unsupported string escape `\\{escaped}`"),
                        offset: offset - 1,
                    });
                }
                offset += escaped.len_utf8();
            }
            '{' if source[offset..].starts_with('{') => offset += 1,
            '{' => interpolation += 1,
            '}' if source[offset..].starts_with('}') => offset += 1,
            '}' if interpolation > 0 => interpolation -= 1,
            '}' => {
                return Err(LiteralError {
                    message: "unmatched `}` in string".into(),
                    offset: offset - 1,
                });
            }
            '"' if interpolation == 0 => {
                closed = true;
                break;
            }
            _ => {}
        }
    }
    if closed && interpolation == 0 {
        Ok(())
    } else {
        Err(LiteralError {
            message: "unterminated string literal or interpolation".into(),
            offset: 0,
        })
    }
}

fn next_char(source: &str, offset: usize) -> char {
    source[offset..].chars().next().unwrap_or('\0')
}

fn take_while(source: &str, mut offset: usize, predicate: impl Fn(char) -> bool) -> usize {
    while offset < source.len() {
        let ch = next_char(source, offset);
        if !predicate(ch) {
            break;
        }
        offset += ch.len_utf8();
    }
    offset
}

fn scan_identifier(source: &str, start: usize) -> usize {
    let mut offset = start;
    if source[start..].starts_with('$') {
        offset += 1;
    }
    let first = next_char(source, offset);
    offset += first.len_utf8();
    take_while(source, offset, super::is_identifier_continue)
}

fn scan_number(source: &str, start: usize) -> usize {
    let mut offset = take_while(source, start, |ch| ch.is_ascii_digit());
    if source[offset..].starts_with('.')
        && !source[offset..].starts_with("..")
        && source[offset + 1..]
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_digit())
    {
        offset = take_while(source, offset + 1, |ch| ch.is_ascii_digit());
    }
    if source[offset..].starts_with(['e', 'E']) {
        offset += 1;
        if source[offset..].starts_with(['+', '-']) {
            offset += 1;
        }
        offset = take_while(source, offset, |ch| ch.is_ascii_digit());
    }
    offset
}

fn scan_string(source: &str, start: usize) -> usize {
    let mut offset = start + 1;
    while offset < source.len() {
        let ch = next_char(source, offset);
        offset += ch.len_utf8();
        if ch == '\\' {
            if offset < source.len() {
                offset += next_char(source, offset).len_utf8();
            }
        } else if ch == '"' {
            break;
        }
    }
    offset
}

#[cfg(test)]
mod tests {
    use super::{tokenize_lossless, LosslessTokenKind};

    #[test]
    fn lossless_tokens_reconstruct_unicode_and_trivia() {
        let source = "变量: \"# {值}\\n\"  # 注释\r\n";
        let tokens = tokenize_lossless(source);
        assert_eq!(tokens.iter().map(|token| token.text(source)).collect::<String>(), source);
        assert!(tokens.iter().any(|token| token.kind == LosslessTokenKind::Comment));
        assert!(tokens.iter().any(|token| token.kind == LosslessTokenKind::Newline));
    }
}
