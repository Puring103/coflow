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

/// 只统计结构分隔符；字符串、注释和空白由无损 token 层统一屏蔽。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct DelimiterNesting {
    braces: u64,
    brackets: u64,
    parentheses: u64,
}

impl DelimiterNesting {
    pub(crate) const fn is_top_level(self) -> bool {
        self.braces == 0 && self.brackets == 0 && self.parentheses == 0
    }

    pub(crate) fn consume(&mut self, token: LosslessToken, source: &str) {
        if token.kind != LosslessTokenKind::Symbol {
            return;
        }
        for ch in token.text(source).chars() {
            match ch {
                '{' => self.braces = self.braces.saturating_add(1),
                '}' => self.braces = self.braces.saturating_sub(1),
                '[' => self.brackets = self.brackets.saturating_add(1),
                ']' => self.brackets = self.brackets.saturating_sub(1),
                '(' => self.parentheses = self.parentheses.saturating_add(1),
                ')' => self.parentheses = self.parentheses.saturating_sub(1),
                _ => {}
            }
        }
    }
}

const SYMBOLS: &[&str] = &[
    // 最长匹配：复合赋值必须先于其运算符前缀，避免格式化把一个运算符拆成两个。
    "//=", "**=", "<<=", ">>=", "..=", "->", "=>", "::", "//", "**", "<<", ">>", "<=", ">=", "==", "!=", "&&", "||", "+=",
    "-=", "*=", "/=", "%=", "&=", "|=", "^=", "..", "(", ")", "{", "}", "[", "]", ",", ":", ";", ".", "?", "+", "-", "*",
    "/", "%", "~", "!", "&", "|", "^", "<", ">", "=", "@",
];

/// 跳过连续空白和 `#` 行注释，返回第一个非 trivia 字节位置。
pub(crate) fn scan_trivia(source: &str, start: usize, limit: usize) -> usize {
    let limit = limit.min(source.len());
    let mut offset = start.min(limit);
    loop {
        let before = offset;
        while offset < limit {
            let ch = next_char(source, offset);
            if !ch.is_whitespace() {
                break;
            }
            offset += ch.len_utf8();
        }
        if offset < limit && source[offset..].starts_with('#') {
            offset = scan_line_comment(source, offset, limit);
        }
        if offset == before {
            return offset;
        }
    }
}

fn scan_line_comment(source: &str, start: usize, limit: usize) -> usize {
    let limit = limit.min(source.len());
    let mut offset = (start + 1).min(limit);
    while offset < limit {
        let ch = next_char(source, offset);
        if matches!(ch, '\r' | '\n') {
            break;
        }
        offset += ch.len_utf8();
    }
    offset
}

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
            offset = scan_line_comment(source, offset, source.len());
            LosslessTokenKind::Comment
        } else if source[offset..].starts_with("f\"") {
            offset = scan_template(source, offset).unwrap_or(source.len());
            LosslessTokenKind::String
        } else if ch == '"' {
            offset = scan_string_literal(source, offset, source.len(), false).end;
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
            offset = scan_number_literal(source, offset, source.len()).end;
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
pub struct LiteralError {
    pub message: String,
    pub offset: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NumberLiteralError {
    FractionDigitsMissing,
    ExponentDigitsMissing,
    InvalidSeparator,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NumberLiteralScan {
    pub(crate) end: usize,
    pub(crate) raw_end: usize,
    pub(crate) is_float: bool,
    pub(crate) error: Option<(NumberLiteralError, usize)>,
}

/// 扫描数字的唯一边界规则；语法层只负责数值转换和诊断代码映射。
pub(crate) fn scan_number_literal(source: &str, start: usize, limit: usize) -> NumberLiteralScan {
    let bytes = source.as_bytes();
    let limit = limit.min(source.len());
    let mut end = start;
    let mut error = None;
    let mut has_separator = false;
    while end < limit && (bytes[end].is_ascii_digit() || bytes[end] == b'_') {
        if bytes[end] == b'_' {
            has_separator = true;
            if end == start
                || !bytes[end - 1].is_ascii_digit()
                || !bytes.get(end + 1).is_some_and(u8::is_ascii_digit)
            {
                error = Some((NumberLiteralError::InvalidSeparator, end));
            }
        }
        end += 1;
    }

    let mut is_float = false;
    if bytes.get(end) == Some(&b'.') && end < limit {
        if bytes.get(end + 1).is_some_and(u8::is_ascii_digit) && end + 1 < limit {
            is_float = true;
            end += 1;
            while end < limit && bytes[end].is_ascii_digit() {
                end += 1;
            }
        } else if bytes.get(end + 1) != Some(&b'.')
            && !source.get(end + 1..limit).and_then(|tail|tail.chars().next()).is_some_and(|ch| ch == '_' || ch.is_alphabetic())
        {
            error = Some((NumberLiteralError::FractionDigitsMissing, end + 1));
        }
    }

    if matches!(bytes.get(end), Some(b'e' | b'E')) && end < limit {
        is_float = true;
        end += 1;
        if matches!(bytes.get(end), Some(b'+' | b'-')) && end < limit {
            end += 1;
        }
        let digits_start = end;
        while end < limit && bytes[end].is_ascii_digit() {
            end += 1;
        }
        if digits_start == end {
            error = Some((NumberLiteralError::ExponentDigitsMissing, end));
        }
    }

    let raw_end = end;
    if is_float && has_separator {
        error = Some((NumberLiteralError::InvalidSeparator, start));
    }

    NumberLiteralScan {
        end,
        raw_end,
        is_float,
        error,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StringLiteralError {
    InvalidEscape { offset: usize, escaped: char },
    Unterminated { end: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StringLiteralScan {
    pub(crate) end: usize,
    pub(crate) contains_format_brace: bool,
    pub(crate) error: Option<StringLiteralError>,
}

/// 扫描普通引号字符串并统一基础转义规则。`stop_at_newline` 由宿主文法决定。
pub(crate) fn scan_string_literal(
    source: &str,
    start: usize,
    limit: usize,
    stop_at_newline: bool,
) -> StringLiteralScan {
    let limit = limit.min(source.len());
    let mut offset = start + 1;
    let mut contains_format_brace = false;
    let mut error = None;
    while offset < limit {
        let ch = next_char(source, offset);
        if stop_at_newline && matches!(ch, '\r' | '\n') {
            return StringLiteralScan {
                end: offset,
                contains_format_brace,
                error: error.or(Some(StringLiteralError::Unterminated { end: offset })),
            };
        }
        offset += ch.len_utf8();
        if ch == '\\' {
            if offset >= limit {
                return StringLiteralScan {
                    end: offset,
                    contains_format_brace,
                    error: error.or(Some(StringLiteralError::Unterminated { end: offset })),
                };
            }
            let escaped = next_char(source, offset);
            if escaped == 'u' {
                if let Ok((_, end)) = unicode_escape(source, offset, limit) {
                    offset = end;
                    continue;
                }
            }
            if decode_simple_escape(escaped).is_none() && error.is_none() {
                error = Some(StringLiteralError::InvalidEscape {
                    offset: offset - 1,
                    escaped,
                });
            }
            offset += escaped.len_utf8();
        } else if ch == '"' {
            return StringLiteralScan {
                end: offset,
                contains_format_brace,
                error,
            };
        } else if matches!(ch, '{' | '}') {
            if source[offset..limit].starts_with(ch) {
                offset += ch.len_utf8();
            } else {
                contains_format_brace = true;
            }
        }
    }
    StringLiteralScan {
        end: limit,
        contains_format_brace,
        error: error.or(Some(StringLiteralError::Unterminated { end: limit })),
    }
}

pub(crate) const fn decode_simple_escape(escaped: char) -> Option<char> {
    match escaped {
        '"' => Some('"'),
        '\\' => Some('\\'),
        'n' => Some('\n'),
        'r' => Some('\r'),
        't' => Some('\t'),
        _ => None,
    }
}

/// 在无损 token 流上匹配分隔符，字符串和注释中的符号不会影响深度。
pub(crate) fn scan_balanced_delimiter(
    source: &str,
    open_offset: usize,
    open: char,
    close: char,
) -> Option<usize> {
    let fragment = &source[open_offset..];
    let mut depth = 0_usize;
    for token in tokenize_lossless(fragment) {
        if token.kind != LosslessTokenKind::Symbol {
            continue;
        }
        for symbol in token.text(fragment).chars() {
            if symbol == open {
                depth += 1;
            } else if symbol == close {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(open_offset + token.span.end);
                }
            }
        }
    }
    None
}

pub fn validate_number_literal(source: &str) -> Result<(), LiteralError> {
    let scan = scan_number_literal(source, 0, source.len());
    if let Some((_, offset)) = scan.error {
        Err(LiteralError {
            message: "expected exponent digits".into(),
            offset,
        })
    } else if scan.end != source.len() || !source.as_bytes().first().is_some_and(u8::is_ascii_digit)
    {
        Err(LiteralError {
            message: "invalid number literal".into(),
            offset: scan.end,
        })
    } else {
        Ok(())
    }
}

/// 按显式 float 期望类型读取字面量，普通整数仍受 i32 范围约束。
pub fn parse_float_literal(source: &str) -> Result<f32, LiteralError> {
    match source {
        "inf" => return Ok(f32::INFINITY),
        "-inf" => return Ok(f32::NEG_INFINITY),
        _ => {}
    }
    let unsigned = source.strip_prefix('-').unwrap_or(source);
    validate_number_literal(unsigned)?;
    let normalized = source.replace('_', "");
    if unsigned.contains('.') || unsigned.contains('e') || unsigned.contains('E') {
        normalized.parse::<f32>().map_err(|_| LiteralError {
            message: "invalid float literal".into(),
            offset: 0,
        })
    } else {
        normalized
            .parse::<i32>()
            .map(|value| value as f32)
            .map_err(|_| LiteralError {
                message: "int outside i32 range".into(),
                offset: 0,
            })
    }
}

/// 校验函数语言使用的格式化字符串转义与插值括号。
pub fn validate_formatted_string_literal(source: &str) -> Result<(), LiteralError> {
    if !source.starts_with("f\"") {
        return Err(LiteralError {
            message: "expected fstring literal".into(),
            offset: 0,
        });
    }
    let end = scan_template(source, 0)?;
    if end != source.len() {
        return Err(LiteralError {
            message: "trailing fstring content".into(),
            offset: end,
        });
    }
    Ok(())
}

fn next_char(source: &str, offset: usize) -> char {
    source[offset..].chars().next().unwrap_or('\0')
}

fn unicode_escape(source: &str, start: usize, limit: usize) -> Result<(char, usize), LiteralError> {
    let fail = || LiteralError {
        message: "invalid Unicode scalar escape".into(),
        offset: start,
    };
    let suffix = source.get(start..limit).ok_or_else(fail)?;
    let digits = suffix.strip_prefix("u{").ok_or_else(fail)?;
    let end = digits.find('}').ok_or_else(fail)?;
    if end == 0 || end > 6 {
        return Err(fail());
    }
    let value = u32::from_str_radix(&digits[..end], 16)
        .ok()
        .and_then(char::from_u32)
        .ok_or_else(fail)?;
    Ok((value, start + 2 + end + 1))
}

/// 字符串转义统一解码为 Unicode 标量，普通字符串的花括号不做模板处理。
pub fn decode_string(source: &str) -> Result<String, LiteralError> {
    if !source.starts_with('"') || !source.ends_with('"') || source.len() < 2 {
        return Err(LiteralError {
            message: "expected quoted string".into(),
            offset: 0,
        });
    }
    let mut result = String::new();
    let mut pos = 1;
    let end = source.len() - 1;
    while pos < end {
        let ch = next_char(source, pos);
        pos += ch.len_utf8();
        if ch == '\\' {
            if pos >= end {
                return Err(LiteralError {
                    message: "unterminated escape".into(),
                    offset: pos,
                });
            }
            let escaped = next_char(source, pos);
            if escaped == 'u' {
                let (value, next) = unicode_escape(source, pos, end)?;
                result.push(value);
                pos = next;
            } else {
                result.push(decode_simple_escape(escaped).ok_or_else(|| LiteralError {
                    message: "invalid escape".into(),
                    offset: pos,
                })?);
                pos += escaped.len_utf8();
            }
        } else if ch == '"' {
            return Err(LiteralError {
                message: "unescaped quote".into(),
                offset: pos - 1,
            });
        } else {
            result.push(ch);
        }
    }
    Ok(result)
}

/// 只确定模板与插值边界，不编译或执行插值表达式。
pub fn scan_template(source: &str, start: usize) -> Result<usize, LiteralError> {
    let mut pos = start + 2;
    let mut depth = 0usize;
    while pos < source.len() {
        let ch = next_char(source, pos);
        if depth > 0 && ch == '"' {
            let scan = scan_string_literal(source, pos, source.len(), false);
            if scan.error.is_some() {
                return Err(LiteralError {
                    message: "invalid string in interpolation".into(),
                    offset: pos,
                });
            }
            pos = scan.end;
            continue;
        }
        if depth > 0 && ch == '#' {
            pos = scan_line_comment(source, pos, source.len());
            continue;
        }
        if depth > 0 && source[pos..].starts_with("f\"") {
            pos = scan_template(source, pos)?;
            continue;
        }
        if depth == 0 && ch == '\\' {
            pos += 1;
            if next_char(source, pos) == 'u' {
                pos = unicode_escape(source, pos, source.len())?.1;
            } else {
                let escaped = next_char(source, pos);
                if decode_simple_escape(escaped).is_none() {
                    return Err(LiteralError {
                        message: "invalid template escape".into(),
                        offset: pos,
                    });
                }
                pos += escaped.len_utf8();
            }
            continue;
        }
        if depth == 0 && (source[pos..].starts_with("{{") || source[pos..].starts_with("}}")) {
            pos += 2;
            continue;
        }
        pos += ch.len_utf8();
        match ch {
            '"' if depth == 0 => return Ok(pos),
            '{' => depth += 1,
            '}' if depth > 0 => depth -= 1,
            '}' => {
                return Err(LiteralError {
                    message: "unmatched template brace".into(),
                    offset: pos - 1,
                })
            }
            _ => {}
        }
    }
    Err(LiteralError {
        message: "unterminated template".into(),
        offset: start,
    })
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

#[cfg(test)]
mod tests {
    use super::{
        scan_balanced_delimiter, scan_number_literal, scan_trivia, tokenize_lossless,
        DelimiterNesting, LosslessTokenKind,
    };

    #[test]
    fn lossless_tokens_reconstruct_unicode_and_trivia() {
        let source = "变量: \"# {值}\\n\"  # 注释\r\n";
        let tokens = tokenize_lossless(source);
        assert_eq!(
            tokens
                .iter()
                .map(|token| token.text(source))
                .collect::<String>(),
            source
        );
        assert!(tokens
            .iter()
            .any(|token| token.kind == LosslessTokenKind::Comment));
        assert!(tokens
            .iter()
            .any(|token| token.kind == LosslessTokenKind::Newline));
    }

    #[test]
    fn shared_scanners_keep_language_boundaries() {
        let number_source = "12.5e-2f next";
        let number = scan_number_literal(number_source, 0, number_source.len());
        assert_eq!(number.end, 7);
        assert!(number.is_float);
        let delimited = "(\" ) \", (# )\n value))";
        assert_eq!(
            scan_balanced_delimiter(delimited, 0, '(', ')'),
            Some(delimited.len())
        );
    }

    #[test]
    fn numeric_boundaries_cover_valid_and_schema_free_neighbors() {
        for (source, end) in [
            ("12,", 2),
            ("12.5]", 4),
            ("12e-3f ", 5),
            ("12..3", 2),
            ("12.", 2),
            ("12e+", 4),
            ("12f32", 2),
        ] {
            assert_eq!(
                scan_number_literal(source, 0, source.len()).end,
                end,
                "{source}"
            );
        }
    }

    #[test]
    fn trivia_and_nesting_share_comment_and_string_boundaries() {
        let source = " \t# { [ ( ignored\r\nnext";
        assert_eq!(
            scan_trivia(source, 0, source.len()),
            source.find("next").unwrap_or(source.len())
        );

        let nested = "{ [ (\"}])\" # }])\n) ] }";
        let mut nesting = DelimiterNesting::default();
        for token in tokenize_lossless(nested) {
            nesting.consume(token, nested);
        }
        assert!(nesting.is_top_level());
    }
}
