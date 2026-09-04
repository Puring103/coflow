use super::super::CfdSyntaxDiagnostic;
use super::Parser;
use crate::lexical::{
    decode_simple_escape, scan_number_literal, scan_string_literal, scan_trivia, StringLiteralError,
};
use crate::Span;

pub(super) struct Token {
    pub(super) text: String,
    pub(super) span: Span,
}

impl Parser<'_> {
    pub(super) fn parse_key(&mut self, label: &str) -> Result<Token, CfdSyntaxDiagnostic> {
        self.skip_ws_and_comments();
        if self.peek_char() == Some('"') {
            let start = self.pos;
            let s = self.parse_quoted_string()?;
            return Ok(Token {
                text: s,
                span: Span::new(start, self.pos),
            });
        }
        self.parse_name_token(label)
    }

    pub(super) fn parse_name(&mut self, label: &str) -> Result<String, CfdSyntaxDiagnostic> {
        self.parse_name_token(label).map(|t| t.text)
    }

    pub(super) fn parse_name_token(&mut self, label: &str) -> Result<Token, CfdSyntaxDiagnostic> {
        self.parse_unquoted_token(label, false)
    }

    pub(super) fn parse_value_token(&mut self, label: &str) -> Result<Token, CfdSyntaxDiagnostic> {
        self.parse_unquoted_token(label, true)
    }

    fn parse_unquoted_token(
        &mut self,
        label: &str,
        scan_numeric_prefix: bool,
    ) -> Result<Token, CfdSyntaxDiagnostic> {
        self.skip_ws_and_comments();
        let start = self.pos;
        if scan_numeric_prefix && self.peek_char().is_some_and(|ch| ch.is_ascii_digit()) {
            // CFD 保留 schema-free 标量文本；共享扫描器只统一合法数字前缀的边界。
            self.pos = scan_number_literal(self.source, start, self.source.len()).end;
        }
        while let Some(ch) = self.peek_char() {
            if ch == ':' && self.source[self.pos..].starts_with("::") {
                self.pos += 2;
                continue;
            }
            if ch.is_whitespace()
                || matches!(
                    ch,
                    ':' | '='
                        | ';'
                        | ','
                        | '{'
                        | '}'
                        | '['
                        | ']'
                        | '('
                        | ')'
                        | '@'
                        | '&'
                        | '|'
                        | '^'
                        | '"'
                )
            {
                break;
            }
            self.pos += ch.len_utf8();
        }
        if self.pos == start {
            return Err(CfdSyntaxDiagnostic {
                message: format!("{label} is missing"),
                span: Span::new(start, start),
            });
        }
        Ok(Token {
            text: self.source[start..self.pos].to_string(),
            span: Span::new(start, self.pos),
        })
    }

    pub(super) fn parse_ref_name(&mut self, label: &str) -> Result<String, CfdSyntaxDiagnostic> {
        self.skip_ws_and_comments();
        let start = self.pos;
        while let Some(ch) = self.peek_char() {
            if ch == ':' && self.source[self.pos..].starts_with("::") {
                self.pos += 2;
                continue;
            }
            if ch.is_whitespace()
                || matches!(
                    ch,
                    '.' | '[' | ']' | ',' | ';' | '}' | ')' | ':' | '@' | '&'
                )
            {
                break;
            }
            self.pos += ch.len_utf8();
        }
        if self.pos == start {
            return Err(CfdSyntaxDiagnostic {
                message: format!("{label} is missing"),
                span: Span::new(start, start),
            });
        }
        Ok(self.source[start..self.pos].to_string())
    }

    pub(super) fn parse_quoted_string(&mut self) -> Result<String, CfdSyntaxDiagnostic> {
        self.skip_ws_and_comments();
        let start = self.pos;
        if self.peek_char() != Some('"') {
            return Err(self.error("expected opening `\"`"));
        }
        let scan = scan_string_literal(self.source, start, self.source.len(), false);
        self.pos = scan.end;
        if let Some(error) = scan.error {
            return Err(match error {
                StringLiteralError::InvalidEscape { offset, escaped } => CfdSyntaxDiagnostic {
                    message: format!("unsupported string escape `\\{escaped}`"),
                    span: Span::new(start, offset + 1 + escaped.len_utf8()),
                },
                StringLiteralError::Unterminated { end } => CfdSyntaxDiagnostic {
                    message: "unterminated string".to_string(),
                    span: Span::new(start, end),
                },
            });
        }

        let mut out = String::new();
        let mut offset = start + 1;
        let content_end = scan.end - 1;
        while offset < content_end {
            let ch = self.source[offset..]
                .chars()
                .next()
                .expect("validated string boundary");
            if ch == '\\' {
                offset += 1;
                let escaped = self.source[offset..]
                    .chars()
                    .next()
                    .expect("validated escape");
                out.push(decode_simple_escape(escaped).expect("validated escape"));
                offset += escaped.len_utf8();
            } else {
                out.push(ch);
                offset += ch.len_utf8();
            }
        }
        Ok(out)
    }

    pub(super) fn skip_ws_and_comments(&mut self) {
        self.pos = scan_trivia(self.source, self.pos, self.source.len());
    }

    pub(super) fn expect_char(
        &mut self,
        expected: char,
        label: &str,
    ) -> Result<(), CfdSyntaxDiagnostic> {
        self.skip_ws_and_comments();
        if self.eat_char(expected) {
            Ok(())
        } else {
            Err(self.error(format!("expected {label}")))
        }
    }

    pub(super) fn eat_char(&mut self, expected: char) -> bool {
        if self.peek_char() == Some(expected) {
            self.pos += expected.len_utf8();
            true
        } else {
            false
        }
    }

    pub(super) fn eat_keyword(&mut self, kw: &str) -> bool {
        self.skip_ws_and_comments();
        if !self.source[self.pos..].starts_with(kw) {
            return false;
        }
        let end = self.pos + kw.len();
        if self
            .source
            .get(end..)
            .and_then(|rest| rest.chars().next())
            .is_some_and(|ch| !is_value_boundary(ch))
        {
            return false;
        }
        self.pos = end;
        true
    }

    pub(super) fn peek_keyword(&self, kw: &str) -> bool {
        if !self.source[self.pos..].starts_with(kw) {
            return false;
        }
        let end = self.pos + kw.len();
        self.source
            .get(end..)
            .and_then(|rest| rest.chars().next())
            .is_none_or(is_value_boundary)
    }

    pub(super) fn peek_char(&self) -> Option<char> {
        self.source[self.pos..].chars().next()
    }

    pub(super) fn is_eof(&self) -> bool {
        self.pos >= self.source.len()
    }
}

fn is_value_boundary(ch: char) -> bool {
    ch.is_whitespace()
        || matches!(
            ch,
            ',' | ';' | '}' | ']' | '(' | ')' | '|' | '^' | '&' | ':'
        )
}
