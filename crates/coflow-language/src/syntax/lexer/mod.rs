mod tokens;

use crate::diagnostics::{CftDiagnostic, CftDiagnostics, CftErrorCode};
use crate::lexical::{
    decode_simple_escape, is_identifier_continue, is_identifier_start, scan_number_literal,
    scan_string_literal, NumberLiteralError, StringLiteralError,
};
use crate::module::ModuleId;
use crate::source::Span;
pub use tokens::{Token, TokenKind};

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
                return Err(self.err(
                    CftErrorCode::UnexpectedCharacter,
                    Span::new(self.pos, self.pos + 1),
                    "formatted strings use ordinary quotes; remove the `f` prefix",
                ));
            }
            if ch == '"' && self.allow_formatted_strings && self.string_contains_braces()? {
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
                ':' if self.starts_with("::") => {
                    self.pos += 2;
                    TokenKind::DoubleColon
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
                '-' if self.starts_with("->") => {
                    self.pos += 2;
                    TokenKind::Arrow
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
                value if is_identifier_start(value) => self.lex_word(),
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
            if is_identifier_continue(ch) {
                self.pos += ch.len_utf8();
            } else {
                break;
            }
        }
        match &self.source[start..self.pos] {
            "namespace" => TokenKind::Namespace,
            "use" => TokenKind::Use,
            "as" => TokenKind::As,
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
            text => TokenKind::Ident(text.to_string()),
        }
    }

    fn lex_number(&mut self, start: usize) -> Result<TokenKind, CftDiagnostics> {
        let scan = scan_number_literal(self.source, start, self.end);
        self.pos = scan.end;
        if let Some((error, end)) = scan.error {
            let message = match error {
                NumberLiteralError::FractionDigitsMissing
                | NumberLiteralError::ExponentDigitsMissing => "invalid float literal",
            };
            return Err(self.err(
                CftErrorCode::InvalidFloatLiteral,
                Span::new(start, end),
                message,
            ));
        }

        let raw = &self.source[start..scan.raw_end];
        if scan.is_float {
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
        let scan = scan_string_literal(self.source, start, self.end, true);
        self.pos = scan.end;
        if let Some(error) = scan.error {
            return Err(match error {
                StringLiteralError::InvalidEscape { offset, escaped } => self.err(
                    CftErrorCode::InvalidStringEscape,
                    Span::new(offset, (offset + 1 + escaped.len_utf8()).min(self.end)),
                    "invalid string escape",
                ),
                StringLiteralError::Unterminated { end } => self.err(
                    CftErrorCode::UnterminatedString,
                    Span::new(start, end),
                    "unterminated string literal",
                ),
            });
        }

        let mut out = String::new();
        let mut offset = start + 1;
        let content_end = scan.end - 1;
        while offset < content_end {
            let ch = self.source[offset..].chars().next().expect("validated string boundary");
            match ch {
                '\\' => {
                    offset += 1;
                    let escaped = self.source[offset..].chars().next().expect("validated escape");
                    out.push(decode_simple_escape(escaped).expect("validated escape"));
                    offset += escaped.len_utf8();
                }
                '{' if self.source[offset..content_end].starts_with("{{") => {
                    out.push('{');
                    offset += 2;
                }
                '}' if self.source[offset..content_end].starts_with("}}") => {
                    out.push('}');
                    offset += 2;
                }
                _ => {
                    out.push(ch);
                    offset += ch.len_utf8();
                }
            }
        }
        Ok(TokenKind::String(out))
    }

    #[allow(clippy::too_many_lines)]
    fn lex_formatted_string(&mut self, tokens: &mut Vec<Token>) -> Result<(), CftDiagnostics> {
        let start = self.pos;
        self.pos += 1;
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
                    Self::push_formatted_text(tokens, &mut text, text_start, self.pos);
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
                    Self::push_formatted_text(tokens, &mut text, text_start, self.pos);
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
                    let Some(value) = decode_simple_escape(char::from(escaped)) else {
                        return Err(self.err(
                            CftErrorCode::InvalidStringEscape,
                            Span::new(escape_start, self.pos + 1),
                            "invalid string escape",
                        ));
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

    fn string_contains_braces(&self) -> Result<bool, CftDiagnostics> {
        let start = self.pos;
        let scan = scan_string_literal(self.source, start, self.end, true);
        match scan.error {
            None | Some(StringLiteralError::InvalidEscape { .. }) => {
                Ok(scan.contains_format_brace)
            }
            Some(StringLiteralError::Unterminated { end }) => Err(self.err(
                CftErrorCode::UnterminatedString,
                Span::new(start, end),
                "unterminated string literal",
            )),
        }
    }

    fn push_formatted_text(tokens: &mut Vec<Token>, text: &mut String, start: usize, end: usize) {
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
