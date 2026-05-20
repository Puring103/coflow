use unicode_ident::{is_xid_continue, is_xid_start};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexError {
    pub kind: LexErrorKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexOutput {
    pub tokens: Vec<Token>,
    pub errors: Vec<LexError>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    Import,
    As,
    Local,
    Class,
    Enum,
    Validate,
    Fn,
    Co,
    Var,
    If,
    Else,
    While,
    For,
    In,
    Break,
    Continue,
    Return,
    Throw,
    Try,
    Catch,
    Yield,
    From,
    And,
    Or,
    Not,
    True,
    False,
    Null,
    SelfKw,
    Ident,
    IntLiteral,
    FloatLiteral,
    StringLiteral,
    RawStringLiteral,
    MultilineStringLiteral,
    RawMultilineStringLiteral,
    Eq,
    PlusEq,
    MinusEq,
    StarEq,
    SlashEq,
    PercentEq,
    QuestionQuestionEq,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    EqEq,
    BangEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    QuestionQuestion,
    Dot,
    QuestionDot,
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Comma,
    Colon,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LexErrorKind {
    UnexpectedChar,
    UnterminatedString,
    UnterminatedBlockComment,
    InvalidEscape,
    InvalidNumber,
}

pub fn lex(source: &str) -> LexOutput {
    Lexer::new(source).lex()
}

struct Lexer<'a> {
    source: &'a str,
    pos: usize,
    tokens: Vec<Token>,
    errors: Vec<LexError>,
}

impl<'a> Lexer<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            pos: 0,
            tokens: Vec::new(),
            errors: Vec::new(),
        }
    }

    fn lex(mut self) -> LexOutput {
        while !self.is_eof() {
            let Some(ch) = self.peek_char() else {
                break;
            };

            if is_whitespace(ch) {
                self.bump_char();
                continue;
            }

            let start = self.pos;
            match ch {
                '/' if self.starts_with("//") => self.skip_line_comment(),
                '/' if self.starts_with("/*") => self.skip_block_comment(),
                'r' if self.starts_with("r\"\"\"") => self.scan_multiline_string(true),
                'r' if self.starts_with("r\"") => self.scan_string(true),
                '"' if self.starts_with("\"\"\"") => self.scan_multiline_string(false),
                '"' => self.scan_string(false),
                '0'..='9' => self.scan_number(),
                '_' => self.scan_identifier(),
                c if is_xid_start(c) => self.scan_identifier(),
                _ => self.scan_punct_or_error(start),
            }
        }

        LexOutput {
            tokens: self.tokens,
            errors: self.errors,
        }
    }

    fn is_eof(&self) -> bool {
        self.pos >= self.source.len()
    }

    fn starts_with(&self, needle: &str) -> bool {
        self.source[self.pos..].starts_with(needle)
    }

    fn peek_char(&self) -> Option<char> {
        self.source[self.pos..].chars().next()
    }

    fn bump_char(&mut self) -> Option<char> {
        let ch = self.peek_char()?;
        self.pos += ch.len_utf8();
        Some(ch)
    }

    fn push_token(&mut self, kind: TokenKind, start: usize, end: usize) {
        self.tokens.push(Token {
            kind,
            span: Span { start, end },
        });
    }

    fn push_error(&mut self, kind: LexErrorKind, start: usize, end: usize) {
        self.errors.push(LexError {
            kind,
            span: Span { start, end },
        });
    }

    fn skip_line_comment(&mut self) {
        while let Some(ch) = self.bump_char() {
            if ch == '\n' {
                break;
            }
        }
    }

    fn skip_block_comment(&mut self) {
        let start = self.pos;
        self.pos += 2;
        while !self.is_eof() {
            if self.starts_with("*/") {
                self.pos += 2;
                return;
            }
            self.bump_char();
        }
        self.push_error(LexErrorKind::UnterminatedBlockComment, start, self.pos);
    }

    fn scan_identifier(&mut self) {
        let start = self.pos;
        self.bump_char();

        while let Some(ch) = self.peek_char() {
            if ch == '_' || is_xid_continue(ch) {
                self.bump_char();
            } else {
                break;
            }
        }

        let text = &self.source[start..self.pos];
        let kind = keyword_kind(text).unwrap_or(TokenKind::Ident);
        self.push_token(kind, start, self.pos);
    }

    fn scan_number(&mut self) {
        let start = self.pos;
        let kind = if self.starts_with("0x") || self.starts_with("0X") {
            self.pos += 2;
            self.scan_prefixed_digits(start, 16)
        } else if self.starts_with("0b") || self.starts_with("0B") {
            self.pos += 2;
            self.scan_prefixed_digits(start, 2)
        } else if self.starts_with("0o") || self.starts_with("0O") {
            self.pos += 2;
            self.scan_prefixed_digits(start, 8)
        } else {
            self.scan_decimal_or_float(start)
        };

        if let Some(kind) = kind {
            self.push_token(kind, start, self.pos);
        }
    }

    fn scan_prefixed_digits(&mut self, start: usize, radix: u32) -> Option<TokenKind> {
        let digit_start = self.pos;
        let mut previous_was_digit = false;
        let mut previous_was_underscore = false;
        let mut saw_digit = false;
        let mut invalid = false;

        while let Some(ch) = self.peek_char() {
            if ch == '_' {
                if !previous_was_digit || previous_was_underscore {
                    invalid = true;
                }
                previous_was_digit = false;
                previous_was_underscore = true;
                self.bump_char();
            } else if ch.is_ascii_alphanumeric() {
                if ch.to_digit(radix).is_some() {
                    saw_digit = true;
                    previous_was_digit = true;
                    previous_was_underscore = false;
                } else {
                    invalid = true;
                    previous_was_digit = false;
                    previous_was_underscore = false;
                }
                self.bump_char();
            } else {
                break;
            }
        }

        if !saw_digit || self.pos == digit_start || previous_was_underscore || invalid {
            let end = self.pos.max(digit_start);
            self.push_error(LexErrorKind::InvalidNumber, start, end);
            None
        } else if self.next_starts_identifier() {
            let end = self.consume_identifier_tail();
            self.push_error(LexErrorKind::InvalidNumber, start, end);
            None
        } else {
            Some(TokenKind::IntLiteral)
        }
    }

    fn scan_decimal_or_float(&mut self, start: usize) -> Option<TokenKind> {
        let mut invalid = false;
        self.consume_digits_and_underscores(&mut invalid);
        let mut kind = TokenKind::IntLiteral;

        if self.starts_with(".") && self.peek_next_char() != Some('.') {
            let dot_pos = self.pos;
            self.pos += 1;
            if matches!(self.peek_char(), Some('0'..='9' | '_')) {
                kind = TokenKind::FloatLiteral;
                self.consume_digits_and_underscores(&mut invalid);
            } else {
                self.pos = dot_pos;
            }
        }

        let text = &self.source[start..self.pos];
        if invalid
            || text.ends_with('_')
            || text.contains("__")
            || text.contains("_.")
            || text.contains("._")
        {
            self.push_error(LexErrorKind::InvalidNumber, start, self.pos);
            return None;
        }

        if self.next_starts_identifier() {
            let end = self.consume_identifier_tail();
            self.push_error(LexErrorKind::InvalidNumber, start, end);
            return None;
        }

        Some(kind)
    }

    fn consume_digits_and_underscores(&mut self, invalid: &mut bool) {
        let mut previous_was_digit = false;
        let mut previous_was_underscore = false;

        while let Some(ch) = self.peek_char() {
            match ch {
                '0'..='9' => {
                    previous_was_digit = true;
                    previous_was_underscore = false;
                    self.bump_char();
                }
                '_' => {
                    if !previous_was_digit || previous_was_underscore {
                        *invalid = true;
                    }
                    previous_was_digit = false;
                    previous_was_underscore = true;
                    self.bump_char();
                }
                _ => break,
            }
        }

        if previous_was_underscore {
            *invalid = true;
        }
    }

    fn next_starts_identifier(&self) -> bool {
        matches!(self.peek_char(), Some('_')) || self.peek_char().is_some_and(is_xid_start)
    }

    fn consume_identifier_tail(&mut self) -> usize {
        while let Some(ch) = self.peek_char() {
            if ch == '_' || is_xid_continue(ch) {
                self.bump_char();
            } else {
                break;
            }
        }
        self.pos
    }

    fn peek_next_char(&self) -> Option<char> {
        let mut chars = self.source[self.pos..].chars();
        chars.next()?;
        chars.next()
    }

    fn scan_string(&mut self, raw: bool) {
        let start = self.pos;
        if raw {
            self.pos += 2;
        } else {
            self.pos += 1;
        }

        while let Some(ch) = self.bump_char() {
            match ch {
                '"' => {
                    let kind = if raw {
                        TokenKind::RawStringLiteral
                    } else {
                        TokenKind::StringLiteral
                    };
                    self.push_token(kind, start, self.pos);
                    return;
                }
                '\n' | '\r' => {
                    self.push_error(LexErrorKind::UnterminatedString, start, self.pos);
                    return;
                }
                '\\' if !raw => match self.bump_char() {
                    Some('"' | '\\' | 'n' | 'r' | 't') => {}
                    Some(_) => {
                        self.push_error(LexErrorKind::InvalidEscape, start, self.pos);
                        return;
                    }
                    None => {
                        self.push_error(LexErrorKind::UnterminatedString, start, self.pos);
                        return;
                    }
                },
                _ => {}
            }
        }

        self.push_error(LexErrorKind::UnterminatedString, start, self.pos);
    }

    fn scan_multiline_string(&mut self, raw: bool) {
        let start = self.pos;
        self.pos += if raw { 4 } else { 3 };

        while !self.is_eof() {
            if self.starts_with("\"\"\"") {
                self.pos += 3;
                let kind = if raw {
                    TokenKind::RawMultilineStringLiteral
                } else {
                    TokenKind::MultilineStringLiteral
                };
                self.push_token(kind, start, self.pos);
                return;
            }

            if !raw && self.starts_with("\\") {
                self.pos += 1;
                match self.bump_char() {
                    Some('"' | '\\' | 'n' | 'r' | 't') => {}
                    Some(_) => {
                        self.push_error(LexErrorKind::InvalidEscape, start, self.pos);
                        return;
                    }
                    None => break,
                }
            } else {
                self.bump_char();
            }
        }

        self.push_error(LexErrorKind::UnterminatedString, start, self.pos);
    }

    fn scan_punct_or_error(&mut self, start: usize) {
        let Some(ch) = self.peek_char() else {
            return;
        };

        let token = if self.starts_with("??=") {
            self.pos += 3;
            Some(TokenKind::QuestionQuestionEq)
        } else if self.starts_with("??") {
            self.pos += 2;
            Some(TokenKind::QuestionQuestion)
        } else if self.starts_with("?.") {
            self.pos += 2;
            Some(TokenKind::QuestionDot)
        } else if self.starts_with("+=") {
            self.pos += 2;
            Some(TokenKind::PlusEq)
        } else if self.starts_with("-=") {
            self.pos += 2;
            Some(TokenKind::MinusEq)
        } else if self.starts_with("*=") {
            self.pos += 2;
            Some(TokenKind::StarEq)
        } else if self.starts_with("/=") {
            self.pos += 2;
            Some(TokenKind::SlashEq)
        } else if self.starts_with("%=") {
            self.pos += 2;
            Some(TokenKind::PercentEq)
        } else if self.starts_with("==") {
            self.pos += 2;
            Some(TokenKind::EqEq)
        } else if self.starts_with("!=") {
            self.pos += 2;
            Some(TokenKind::BangEq)
        } else if self.starts_with("<=") {
            self.pos += 2;
            Some(TokenKind::LtEq)
        } else if self.starts_with(">=") {
            self.pos += 2;
            Some(TokenKind::GtEq)
        } else {
            self.pos += ch.len_utf8();
            match ch {
                '=' => Some(TokenKind::Eq),
                '+' => Some(TokenKind::Plus),
                '-' => Some(TokenKind::Minus),
                '*' => Some(TokenKind::Star),
                '/' => Some(TokenKind::Slash),
                '%' => Some(TokenKind::Percent),
                '<' => Some(TokenKind::Lt),
                '>' => Some(TokenKind::Gt),
                '.' => Some(TokenKind::Dot),
                '(' => Some(TokenKind::LParen),
                ')' => Some(TokenKind::RParen),
                '{' => Some(TokenKind::LBrace),
                '}' => Some(TokenKind::RBrace),
                '[' => Some(TokenKind::LBracket),
                ']' => Some(TokenKind::RBracket),
                ',' => Some(TokenKind::Comma),
                ':' => Some(TokenKind::Colon),
                _ => None,
            }
        };

        if let Some(kind) = token {
            self.push_token(kind, start, self.pos);
        } else {
            self.push_error(LexErrorKind::UnexpectedChar, start, self.pos);
        }
    }
}

fn is_whitespace(ch: char) -> bool {
    matches!(ch, ' ' | '\t' | '\n' | '\r' | '\u{000C}')
}

fn keyword_kind(text: &str) -> Option<TokenKind> {
    Some(match text {
        "import" => TokenKind::Import,
        "as" => TokenKind::As,
        "local" => TokenKind::Local,
        "class" => TokenKind::Class,
        "enum" => TokenKind::Enum,
        "validate" => TokenKind::Validate,
        "fn" => TokenKind::Fn,
        "co" => TokenKind::Co,
        "var" => TokenKind::Var,
        "if" => TokenKind::If,
        "else" => TokenKind::Else,
        "while" => TokenKind::While,
        "for" => TokenKind::For,
        "in" => TokenKind::In,
        "break" => TokenKind::Break,
        "continue" => TokenKind::Continue,
        "return" => TokenKind::Return,
        "throw" => TokenKind::Throw,
        "try" => TokenKind::Try,
        "catch" => TokenKind::Catch,
        "yield" => TokenKind::Yield,
        "from" => TokenKind::From,
        "and" => TokenKind::And,
        "or" => TokenKind::Or,
        "not" => TokenKind::Not,
        "true" => TokenKind::True,
        "false" => TokenKind::False,
        "null" => TokenKind::Null,
        "self" => TokenKind::SelfKw,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(source: &str) -> Vec<TokenKind> {
        let output = lex(source);
        assert_eq!(output.errors, []);
        output.tokens.into_iter().map(|token| token.kind).collect()
    }

    fn first_error(source: &str) -> LexErrorKind {
        let output = lex(source);
        output.errors.first().expect("expected lex error").kind
    }

    #[test]
    fn recognizes_keywords_and_identifiers() {
        assert_eq!(
            kinds("import imported className return_value null int"),
            vec![
                TokenKind::Import,
                TokenKind::Ident,
                TokenKind::Ident,
                TokenKind::Ident,
                TokenKind::Null,
                TokenKind::Ident
            ]
        );
    }

    #[test]
    fn recognizes_unicode_identifiers() {
        assert_eq!(
            kinds("玩家生命 скорость Δx 名前 _临时值 _1"),
            vec![
                TokenKind::Ident,
                TokenKind::Ident,
                TokenKind::Ident,
                TokenKind::Ident,
                TokenKind::Ident,
                TokenKind::Ident
            ]
        );
    }

    #[test]
    fn recognizes_longest_match_operators() {
        assert_eq!(
            kinds("??= ?? ?. += -= *= /= %= == != <= >= ."),
            vec![
                TokenKind::QuestionQuestionEq,
                TokenKind::QuestionQuestion,
                TokenKind::QuestionDot,
                TokenKind::PlusEq,
                TokenKind::MinusEq,
                TokenKind::StarEq,
                TokenKind::SlashEq,
                TokenKind::PercentEq,
                TokenKind::EqEq,
                TokenKind::BangEq,
                TokenKind::LtEq,
                TokenKind::GtEq,
                TokenKind::Dot,
            ]
        );
    }

    #[test]
    fn recognizes_number_forms() {
        assert_eq!(
            kinds("0 123 1_000 0xff 0XFF 0b1010_0101 0o755 1.0 1_000.5 1.000_5"),
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
                TokenKind::FloatLiteral
            ]
        );
    }

    #[test]
    fn reports_invalid_numbers() {
        for source in ["1_", "1__0", "0x_ff", "0b102", "0x", "123abc", "1变量", "1e3"] {
            assert_eq!(first_error(source), LexErrorKind::InvalidNumber, "{source}");
        }
    }

    #[test]
    fn recognizes_string_forms() {
        assert_eq!(
            kinds("\"hero\\n\" r\"C:\\game\" \"\"\"line\nline\"\"\" r\"\"\"C:\\game\nline\"\"\""),
            vec![
                TokenKind::StringLiteral,
                TokenKind::RawStringLiteral,
                TokenKind::MultilineStringLiteral,
                TokenKind::RawMultilineStringLiteral,
            ]
        );
    }

    #[test]
    fn reports_string_errors() {
        assert_eq!(first_error("\"bad \\x\""), LexErrorKind::InvalidEscape);
        assert_eq!(first_error("\"unterminated"), LexErrorKind::UnterminatedString);
        assert_eq!(first_error("\"line\nnext\""), LexErrorKind::UnterminatedString);
        assert_eq!(first_error("r\"unterminated"), LexErrorKind::UnterminatedString);
    }

    #[test]
    fn skips_comments_and_reports_unclosed_block_comment() {
        assert_eq!(kinds("a // comment\n b /* hidden */ c").len(), 3);
        assert_eq!(
            first_error("a /* comment"),
            LexErrorKind::UnterminatedBlockComment
        );
    }

    #[test]
    fn reports_unexpected_characters_and_recovers() {
        let output = lex("? | !");
        assert_eq!(
            output.errors.iter().map(|error| error.kind).collect::<Vec<_>>(),
            vec![
                LexErrorKind::UnexpectedChar,
                LexErrorKind::UnexpectedChar,
                LexErrorKind::UnexpectedChar
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
}
