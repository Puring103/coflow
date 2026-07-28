use super::Parser;
use crate::{CfdSyntaxDiagnostic, Span};

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

    fn parse_name_token(&mut self, label: &str) -> Result<Token, CfdSyntaxDiagnostic> {
        self.skip_ws_and_comments();
        let start = self.pos;
        while let Some(ch) = self.peek_char() {
            if ch.is_whitespace()
                || matches!(
                    ch,
                    ':' | '=' | ';' | ',' | '{' | '}' | '[' | ']' | '(' | ')' | '@' | '&' | '"'
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
        self.expect_char('"', "opening `\"`")?;
        let mut out = String::new();
        let mut escaped = false;
        while let Some(ch) = self.peek_char() {
            self.pos += ch.len_utf8();
            if escaped {
                match ch {
                    '"' => out.push('"'),
                    '\\' => out.push('\\'),
                    'n' => out.push('\n'),
                    'r' => out.push('\r'),
                    't' => out.push('\t'),
                    other => {
                        return Err(CfdSyntaxDiagnostic {
                            message: format!("unsupported string escape `\\{other}`"),
                            span: Span::new(start, self.pos),
                        });
                    }
                }
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                return Ok(out);
            } else {
                out.push(ch);
            }
        }
        Err(CfdSyntaxDiagnostic {
            message: "unterminated string".to_string(),
            span: Span::new(start, self.pos),
        })
    }

    pub(super) fn skip_ws_and_comments(&mut self) {
        loop {
            while self.peek_char().is_some_and(char::is_whitespace) {
                self.pos += self.peek_char().map_or(0, char::len_utf8);
            }
            if self.source[self.pos..].starts_with('#') {
                self.pos += 1;
                while self.peek_char().is_some_and(|ch| ch != '\n') {
                    self.pos += self.peek_char().map_or(0, char::len_utf8);
                }
                continue;
            }
            break;
        }
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

    pub(super) fn eat_spread(&mut self) -> bool {
        self.skip_ws_and_comments();
        if self.source[self.pos..].starts_with("...") {
            self.pos += 3;
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
    ch.is_whitespace() || matches!(ch, ',' | ';' | '}' | ']' | '|' | ':')
}
