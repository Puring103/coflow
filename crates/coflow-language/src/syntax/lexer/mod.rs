mod tokens;

use crate::diagnostics::{CftDiagnostic, CftDiagnostics, CftErrorCode};
use crate::lexical::{
    is_identifier_continue, is_identifier_start, scan_number_literal, scan_string_literal,
    scan_trivia, NumberLiteralError, StringLiteralError,
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
    pos: usize,
    end: usize,
}

impl<'a> Lexer<'a> {
    fn new(module: &'a ModuleId, source: &'a str) -> Self {
        Self {
            module,
            source,
            pos: 0,
            end: source.len(),
        }
    }

    #[allow(clippy::too_many_lines)]
    fn lex(mut self) -> Result<Vec<Token>, CftDiagnostics> {
        let mut tokens = Vec::new();
        while self.pos < self.end {
            let Some(ch) = self.source[self.pos..].chars().next() else {
                break;
            };
            if ch.is_whitespace() || ch == '#' {
                self.pos = scan_trivia(self.source, self.pos, self.end);
                continue;
            }
            if ch == 'f' && self.source[self.pos..self.end].starts_with("f\"") {
                let start = self.pos;
                self.pos = crate::lexical::scan_template(self.source, start).map_err(|error| {
                    self.err(
                        CftErrorCode::InvalidStringEscape,
                        Span::new(start, self.end),
                        error.message,
                    )
                })?;
                tokens.push(Token {
                    kind: TokenKind::FormattedStringStart,
                    span: Span::new(start, start + 2),
                });
                tokens.push(Token {
                    kind: TokenKind::FormattedStringEnd,
                    span: Span::new(self.pos - 1, self.pos),
                });
                continue;
            }

            let start = self.pos;
            let kind = match ch {
                '?' => {
                    self.pos += 1;
                    TokenKind::Question
                }
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
            "const" => TokenKind::Const,
            "enum" => TokenKind::Enum,
            "type" => TokenKind::Type,
            "table" => TokenKind::Table,
            "singleton" => TokenKind::Singleton,
            "data" => TokenKind::Data,
            "abstract" => TokenKind::Abstract,
            "sealed" => TokenKind::Sealed,
            "check" => TokenKind::Check,
            "in" => TokenKind::In,
            "is" => TokenKind::Is,
            "inf" => TokenKind::Float(f64::INFINITY),
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
                NumberLiteralError::InvalidSeparator => "invalid numeric separator",
            };
            return Err(self.err(
                CftErrorCode::InvalidFloatLiteral,
                Span::new(start, end),
                message,
            ));
        }

        let normalized = self.source[start..scan.raw_end].replace('_', "");
        let raw = normalized.as_str();
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
        let Ok(value) = raw.parse::<f32>() else {
            return Err(self.err(
                CftErrorCode::InvalidFloatLiteral,
                Span::new(start, self.pos),
                "invalid float literal",
            ));
        };
        Ok(TokenKind::Float(f64::from(value)))
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

        crate::lexical::decode_string(&self.source[start..self.pos])
            .map(TokenKind::String)
            .map_err(|error| {
                self.err(
                    CftErrorCode::InvalidStringEscape,
                    Span::new(start, self.pos),
                    error.message,
                )
            })
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
