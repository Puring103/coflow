mod tokens;

use crate::diagnostics::{CftDiagnostic, CftDiagnostics, CftErrorCode};
use crate::module::ModuleId;
use crate::syntax::Span;
pub use tokens::{Token, TokenKind};
use unicode_ident::{is_xid_continue, is_xid_start};

/// Lexes one CFT module into tokens.
///
/// # Errors
///
/// Returns diagnostics when the source contains invalid characters,
/// unterminated strings, or malformed lexical constructs.
pub fn lex(module: &ModuleId, source: &str) -> Result<Vec<Token>, CftDiagnostics> {
    Lexer::new(module, source).lex()
}

struct Lexer<'a> {
    module: &'a ModuleId,
    source: &'a str,
    bytes: &'a [u8],
    pos: usize,
    end: usize,
    allow_formatted_strings: bool,
}

impl<'a> Lexer<'a> {
    fn new(module: &'a ModuleId, source: &'a str) -> Self {
        Self {
            module,
            source,
            bytes: source.as_bytes(),
            pos: 0,
            end: source.len(),
            allow_formatted_strings: true,
        }
    }

    fn fragment(&self, start: usize, end: usize) -> Self {
        Self {
            module: self.module,
            source: self.source,
            bytes: self.bytes,
            pos: start,
            end,
            allow_formatted_strings: false,
        }
    }

    #[allow(clippy::too_many_lines)]
    fn lex(mut self) -> Result<Vec<Token>, CftDiagnostics> {
        let mut tokens = Vec::new();
        while self.pos < self.end {
            let Some(ch) = self.source[self.pos..].chars().next() else {
                break;
            };
            if ch.is_whitespace() {
                self.pos += ch.len_utf8();
                continue;
            }
            if ch == '#' {
                self.pos += 1;
                while self.pos < self.end && self.bytes[self.pos] != b'\n' {
                    self.pos += 1;
                }
                continue;
            }
            if ch == 'f' && self.source[self.pos..self.end].starts_with("f\"") {
                if !self.allow_formatted_strings {
                    return Err(self.err(
                        CftErrorCode::UnexpectedCharacter,
                        Span::new(self.pos, self.pos + 2),
                        "nested formatted strings are not supported",
                    ));
                }
                self.lex_formatted_string(&mut tokens)?;
                continue;
            }

            let start = self.pos;
            let kind = match ch {
                '@' => {
                    self.pos += 1;
                    TokenKind::At
                }
                '{' => {
                    self.pos += 1;
                    TokenKind::LBrace
                }
                '}' => {
                    self.pos += 1;
                    TokenKind::RBrace
                }
                '[' => {
                    self.pos += 1;
                    TokenKind::LBracket
                }
                ']' => {
                    self.pos += 1;
                    TokenKind::RBracket
                }
                '(' => {
                    self.pos += 1;
                    TokenKind::LParen
                }
                ')' => {
                    self.pos += 1;
                    TokenKind::RParen
                }
                ':' => {
                    self.pos += 1;
                    TokenKind::Colon
                }
                ';' => {
                    self.pos += 1;
                    TokenKind::Semicolon
                }
                ',' => {
                    self.pos += 1;
                    TokenKind::Comma
                }
                '.' => {
                    self.pos += 1;
                    TokenKind::Dot
                }
                '+' => {
                    self.pos += 1;
                    TokenKind::Plus
                }
                '-' => {
                    self.pos += 1;
                    TokenKind::Minus
                }
                '*' if self.starts_with("**") => {
                    self.pos += 2;
                    TokenKind::StarStar
                }
                '*' => {
                    self.pos += 1;
                    TokenKind::Star
                }
                '/' if self.starts_with("//") => {
                    self.pos += 2;
                    TokenKind::SlashSlash
                }
                '/' => {
                    self.pos += 1;
                    TokenKind::Slash
                }
                '%' => {
                    self.pos += 1;
                    TokenKind::Percent
                }
                '=' if self.starts_with("==") => {
                    self.pos += 2;
                    TokenKind::EqEq
                }
                '=' => {
                    self.pos += 1;
                    TokenKind::Equal
                }
                '<' if self.starts_with("<=") => {
                    self.pos += 2;
                    TokenKind::LessEq
                }
                '<' if self.starts_with("<<") => {
                    self.pos += 2;
                    TokenKind::LessLess
                }
                '<' => {
                    self.pos += 1;
                    TokenKind::Less
                }
                '>' if self.starts_with(">=") => {
                    self.pos += 2;
                    TokenKind::GreaterEq
                }
                '>' if self.starts_with(">>") => {
                    self.pos += 2;
                    TokenKind::GreaterGreater
                }
                '>' => {
                    self.pos += 1;
                    TokenKind::Greater
                }
                '!' if self.starts_with("!=") => {
                    self.pos += 2;
                    TokenKind::BangEq
                }
                '!' => {
                    self.pos += 1;
                    TokenKind::Bang
                }
                '?' => {
                    self.pos += 1;
                    TokenKind::Question
                }
                '&' if self.starts_with("&&") => {
                    self.pos += 2;
                    TokenKind::AmpAmp
                }
                '&' => {
                    self.pos += 1;
                    TokenKind::Amp
                }
                '|' if self.starts_with("||") => {
                    self.pos += 2;
                    TokenKind::PipePipe
                }
                '|' => {
                    self.pos += 1;
                    TokenKind::Pipe
                }
                '^' => {
                    self.pos += 1;
                    TokenKind::Caret
                }
                '~' => {
                    self.pos += 1;
                    TokenKind::Tilde
                }
                '"' => self.lex_string(start)?,
                '0'..='9' => self.lex_number(start)?,
                '_' => self.lex_word(),
                value if is_xid_start(value) => self.lex_word(),
                _ => {
                    return Err(self.err(
                        CftErrorCode::UnexpectedCharacter,
                        Span::new(start, start + ch.len_utf8()),
                        format!("unexpected character `{ch}`"),
                    ));
                }
            };
            tokens.push(Token {
                kind,
                span: Span::new(start, self.pos),
            });
        }
        tokens.push(Token {
            kind: TokenKind::Eof,
            span: Span::new(self.end, self.end),
        });
        Ok(tokens)
    }

    fn lex_word(&mut self) -> TokenKind {
        let start = self.pos;
        while let Some(ch) = self.source[self.pos..].chars().next() {
            if ch == '_' || is_xid_continue(ch) {
                self.pos += ch.len_utf8();
            } else {
                break;
            }
        }
        match &self.source[start..self.pos] {
            "const" => TokenKind::Const,
            "enum" => TokenKind::Enum,
            "type" => TokenKind::Type,
            "abstract" => TokenKind::Abstract,
            "sealed" => TokenKind::Sealed,
            "check" => TokenKind::Check,
            "when" => TokenKind::When,
            "all" => TokenKind::All,
            "any" => TokenKind::Any,
            "none" => TokenKind::None,
            "in" => TokenKind::In,
            "is" => TokenKind::Is,
            "true" => TokenKind::True,
            "false" => TokenKind::False,
            "null" => TokenKind::Null,
            text => TokenKind::Ident(text.to_string()),
        }
    }

    fn lex_number(&mut self, start: usize) -> Result<TokenKind, CftDiagnostics> {
        while self.pos < self.end && self.bytes[self.pos].is_ascii_digit() {
            self.pos += 1;
        }

        let mut is_float = false;
        if self.bytes.get(self.pos) == Some(&b'.') {
            if self.bytes.get(self.pos + 1).is_some_and(u8::is_ascii_digit) {
                self.pos += 1;
                while self.pos < self.end && self.bytes[self.pos].is_ascii_digit() {
                    self.pos += 1;
                }
                is_float = true;
            } else {
                return Err(self.err(
                    CftErrorCode::InvalidFloatLiteral,
                    Span::new(start, self.pos + 1),
                    "invalid float literal",
                ));
            }
        }

        if matches!(self.bytes.get(self.pos), Some(b'e' | b'E')) {
            let exp_start = self.pos;
            self.pos += 1;
            if matches!(self.bytes.get(self.pos), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            let digits_start = self.pos;
            while self.pos < self.end && self.bytes[self.pos].is_ascii_digit() {
                self.pos += 1;
            }
            if digits_start == self.pos {
                return Err(self.err(
                    CftErrorCode::InvalidFloatLiteral,
                    Span::new(start, self.pos.max(exp_start + 1)),
                    "invalid float literal",
                ));
            }
            is_float = true;
        }

        let raw_end = self.pos;
        if matches!(self.bytes.get(self.pos), Some(b'f' | b'F')) {
            let suffix_end = self.pos + 1;
            let next = self.source[suffix_end..].chars().next();
            if !next.is_some_and(|ch| ch == '_' || is_xid_continue(ch)) {
                self.pos = suffix_end;
                is_float = true;
            }
        }

        let raw = &self.source[start..raw_end];
        if is_float {
            self.lex_float(raw, start)
        } else if let Ok(value) = raw.parse::<i64>() {
            Ok(TokenKind::Int(value))
        } else if let Ok(value) = raw.parse::<u64>() {
            // The magnitude doesn't fit in i64 but does fit in u64. This is
            // legal only when followed by a unary `-` in the parser; standalone
            // it will raise `InvalidIntLiteral` there.
            Ok(TokenKind::UIntOverflow(value))
        } else {
            Err(self.err(
                CftErrorCode::InvalidIntLiteral,
                Span::new(start, self.pos),
                "invalid int literal",
            ))
        }
    }

    fn lex_float(&self, raw: &str, start: usize) -> Result<TokenKind, CftDiagnostics> {
        let Ok(value) = raw.parse::<f64>() else {
            return Err(self.err(
                CftErrorCode::InvalidFloatLiteral,
                Span::new(start, self.pos),
                "invalid float literal",
            ));
        };
        if !value.is_finite() {
            return Err(self.err(
                CftErrorCode::InvalidFloatLiteral,
                Span::new(start, self.pos),
                "float literal must be finite",
            ));
        }
        Ok(TokenKind::Float(value))
    }

    fn lex_string(&mut self, start: usize) -> Result<TokenKind, CftDiagnostics> {
        self.pos += 1;
        let mut out = String::new();
        while self.pos < self.end {
            let Some(ch) = self.source[self.pos..].chars().next() else {
                break;
            };
            match ch {
                '"' => {
                    self.pos += 1;
                    return Ok(TokenKind::String(out));
                }
                '\\' => {
                    let escape_start = self.pos;
                    self.pos += 1;
                    let Some(escaped) = self.bytes.get(self.pos).copied() else {
                        break;
                    };
                    let value = match escaped {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        _ => {
                            return Err(self.err(
                                CftErrorCode::InvalidStringEscape,
                                Span::new(escape_start, self.pos + 1),
                                "invalid string escape",
                            ));
                        }
                    };
                    out.push(value);
                    self.pos += 1;
                }
                '\n' | '\r' => {
                    return Err(self.err(
                        CftErrorCode::UnterminatedString,
                        Span::new(start, self.pos),
                        "unterminated string literal",
                    ));
                }
                _ => {
                    out.push(ch);
                    self.pos += ch.len_utf8();
                }
            }
        }
        Err(self.err(
            CftErrorCode::UnterminatedString,
            Span::new(start, self.end),
            "unterminated string literal",
        ))
    }

    fn lex_formatted_string(
        &mut self,
        tokens: &mut Vec<Token>,
    ) -> Result<(), CftDiagnostics> {
        let start = self.pos;
        self.pos += 2;
        tokens.push(Token {
            kind: TokenKind::FormattedStringStart,
            span: Span::new(start, self.pos),
        });
        let mut text = String::new();
        let mut text_start = self.pos;
        while self.pos < self.end {
            let Some(ch) = self.source[self.pos..].chars().next() else {
                break;
            };
            match ch {
                '"' => {
                    self.push_formatted_text(tokens, &mut text, text_start, self.pos);
                    let quote = self.pos;
                    self.pos += 1;
                    tokens.push(Token {
                        kind: TokenKind::FormattedStringEnd,
                        span: Span::new(quote, self.pos),
                    });
                    return Ok(());
                }
                '{' if self.starts_with("{{") => {
                    text.push('{');
                    self.pos += 2;
                }
                '}' if self.starts_with("}}") => {
                    text.push('}');
                    self.pos += 2;
                }
                '{' => {
                    self.push_formatted_text(tokens, &mut text, text_start, self.pos);
                    let opener = self.pos;
                    self.pos += 1;
                    tokens.push(Token {
                        kind: TokenKind::FormattedStringExprStart,
                        span: Span::new(opener, self.pos),
                    });
                    let expr_start = self.pos;
                    let expr_end = self.find_formatted_expr_end(expr_start)?;
                    if expr_start == expr_end {
                        return Err(self.err(
                            CftErrorCode::InvalidCheckStatement,
                            Span::new(opener, expr_end + 1),
                            "formatted string interpolation cannot be empty",
                        ));
                    }
                    let mut expression_tokens = self.fragment(expr_start, expr_end).lex()?;
                    let _ = expression_tokens.pop();
                    tokens.extend(expression_tokens);
                    tokens.push(Token {
                        kind: TokenKind::FormattedStringExprEnd,
                        span: Span::new(expr_end, expr_end + 1),
                    });
                    self.pos = expr_end + 1;
                    text_start = self.pos;
                }
                '}' => {
                    return Err(self.err(
                        CftErrorCode::UnexpectedCharacter,
                        Span::new(self.pos, self.pos + 1),
                        "literal `}` in a formatted string must be written as `}}`",
                    ));
                }
                '\\' => {
                    let escape_start = self.pos;
                    self.pos += 1;
                    let Some(escaped) = self.bytes.get(self.pos).copied() else {
                        break;
                    };
                    let value = match escaped {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        _ => {
                            return Err(self.err(
                                CftErrorCode::InvalidStringEscape,
                                Span::new(escape_start, self.pos + 1),
                                "invalid string escape",
                            ));
                        }
                    };
                    text.push(value);
                    self.pos += 1;
                }
                '\n' | '\r' => {
                    return Err(self.err(
                        CftErrorCode::UnterminatedString,
                        Span::new(start, self.pos),
                        "unterminated formatted string literal",
                    ));
                }
                _ => {
                    text.push(ch);
                    self.pos += ch.len_utf8();
                }
            }
        }
        Err(self.err(
            CftErrorCode::UnterminatedString,
            Span::new(start, self.end),
            "unterminated formatted string literal",
        ))
    }

    fn push_formatted_text(
        &self,
        tokens: &mut Vec<Token>,
        text: &mut String,
        start: usize,
        end: usize,
    ) {
        if text.is_empty() {
            return;
        }
        tokens.push(Token {
            kind: TokenKind::FormattedStringText(std::mem::take(text)),
            span: Span::new(start, end),
        });
    }

    fn find_formatted_expr_end(&self, start: usize) -> Result<usize, CftDiagnostics> {
        let mut pos = start;
        let mut brace_depth = 0_usize;
        let mut in_string = false;
        let mut escaped = false;
        while pos < self.end {
            let Some(ch) = self.source[pos..].chars().next() else {
                break;
            };
            if in_string {
                if escaped {
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == '"' {
                    in_string = false;
                } else if matches!(ch, '\n' | '\r') {
                    return Err(self.err(
                        CftErrorCode::UnterminatedString,
                        Span::new(start, pos),
                        "unterminated string in formatted interpolation",
                    ));
                }
            } else {
                match ch {
                    '"' => in_string = true,
                    '{' => brace_depth += 1,
                    '}' if brace_depth == 0 => return Ok(pos),
                    '}' => brace_depth -= 1,
                    '#' | '\n' | '\r' => {
                        return Err(self.err(
                            CftErrorCode::InvalidCheckStatement,
                            Span::new(pos, pos + ch.len_utf8()),
                            "formatted string interpolation must stay on one line and cannot contain comments",
                        ));
                    }
                    _ => {}
                }
            }
            pos += ch.len_utf8();
        }
        Err(self.err(
            CftErrorCode::UnterminatedString,
            Span::new(start.saturating_sub(1), self.end),
            "unterminated formatted string interpolation",
        ))
    }

    fn starts_with(&self, text: &str) -> bool {
        self.source[self.pos..self.end].starts_with(text)
    }

    fn err(&self, code: CftErrorCode, span: Span, message: impl Into<String>) -> CftDiagnostics {
        CftDiagnostics::one(CftDiagnostic::error(
            code,
            self.module.clone(),
            span,
            message,
        ))
    }
}
