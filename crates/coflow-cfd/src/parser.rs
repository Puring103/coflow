mod tokens;

use crate::ast::{CfdAst, CfdBlock, CfdBlockEntry, CfdField, CfdRecord, CfdRef, CfdValue};
use crate::CfdSyntaxDiagnostic;
use coflow_cft::Span;
use tokens::Token;

pub fn parse(source: &str) -> (CfdAst, Vec<CfdSyntaxDiagnostic>) {
    let mut p = Parser::new(source);
    let ast = p.parse_root();
    (ast, p.diagnostics)
}

struct Parser<'a> {
    source: &'a str,
    pos: usize,
    pub diagnostics: Vec<CfdSyntaxDiagnostic>,
}

impl<'a> Parser<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            pos: 0,
            diagnostics: Vec::new(),
        }
    }

    fn parse_root(&mut self) -> CfdAst {
        let mut records = Vec::new();
        self.skip_ws_and_comments();
        while !self.is_eof() {
            match self.parse_top_level() {
                Ok(new) => records.extend(new),
                Err(diag) => {
                    self.diagnostics.push(diag);
                    self.recover_to_next_record();
                }
            }
            self.skip_ws_and_comments();
        }
        CfdAst { records }
    }

    /// Skip tokens until we find something that looks like the start of a new
    /// top-level record (an identifier at column 0 context, or EOF).
    fn recover_to_next_record(&mut self) {
        while !self.is_eof() {
            // Consume until end of line or `}` that could close a group block.
            while let Some(ch) = self.peek_char() {
                if ch == '\n' {
                    self.pos += 1;
                    break;
                }
                self.pos += ch.len_utf8();
            }
            self.skip_ws_and_comments();
            // Stop recovering when the next char starts an identifier (new record).
            if self
                .peek_char()
                .is_some_and(|ch| ch.is_alphabetic() || ch == '"' || ch == '_')
            {
                break;
            }
        }
    }

    fn parse_top_level(&mut self) -> Result<Vec<CfdRecord>, CfdSyntaxDiagnostic> {
        let first = self.parse_key("record key or group type")?;
        self.skip_ws_and_comments();

        if self.eat_char(':') {
            // `key: TypeName { ... }`
            self.skip_ws_and_comments();
            let type_start = self.pos;
            let type_name = self.parse_name("record type")?;
            let type_span = Span::new(type_start, self.pos);
            let block = self.parse_block()?;
            let span = Span::new(first.span.start, block.span.end);
            let (entries, fields) = block.into_entries_and_fields();
            Ok(vec![CfdRecord {
                key: first.text,
                key_span: first.span,
                group_type: None,
                type_name,
                type_span,
                entries,
                fields,
                span,
            }])
        } else if self.peek_char() == Some('{') {
            // `GroupType { ... }`
            self.parse_group(&first)
        } else {
            Err(self.error("expected `:` or `{`"))
        }
    }

    fn parse_group(&mut self, group_token: &Token) -> Result<Vec<CfdRecord>, CfdSyntaxDiagnostic> {
        self.expect_char('{', "group body `{`")?;
        let mut records = Vec::new();
        loop {
            self.skip_ws_and_comments();
            if self.eat_char('}') {
                break;
            }
            if self.is_eof() {
                return Err(self.error("unterminated group body, expected `}`"));
            }
            let key = self.parse_key("record key")?;
            self.skip_ws_and_comments();

            let (type_name, type_span) = if self.eat_char(':') {
                self.skip_ws_and_comments();
                let ts = self.pos;
                let name = self.parse_name("record type")?;
                (name, Span::new(ts, self.pos))
            } else {
                (group_token.text.clone(), group_token.span)
            };

            let block = self.parse_block()?;
            let span = Span::new(key.span.start, block.span.end);
            let (entries, fields) = block.into_entries_and_fields();
            records.push(CfdRecord {
                key: key.text,
                key_span: key.span,
                group_type: Some((group_token.text.clone(), group_token.span)),
                type_name,
                type_span,
                entries,
                fields,
                span,
            });

            self.skip_ws_and_comments();
            let _ = self.eat_char(',');
        }
        Ok(records)
    }

    fn parse_block(&mut self) -> Result<CfdBlock, CfdSyntaxDiagnostic> {
        self.skip_ws_and_comments();
        // Optional type marker before `{`
        let type_marker = if self.peek_char() == Some('{') {
            None
        } else {
            let ts = self.pos;
            let name = self.parse_name("block type or `{`")?;
            let name_end = self.pos; // capture before whitespace skip
            self.skip_ws_and_comments();
            if self.peek_char() != Some('{') {
                return Err(self.error("expected `{` after type marker"));
            }
            Some((name, Span::new(ts, name_end)))
        };

        let start = self.pos;
        self.expect_char('{', "block start `{`")?;
        let mut entries = Vec::new();

        loop {
            self.skip_ws_and_comments();
            if self.eat_char('}') {
                break;
            }
            if self.is_eof() {
                return Err(self.error("unterminated block, expected `}`"));
            }

            if self.eat_spread() {
                let spread_start = self.pos - 3;
                let value = self.parse_value()?;
                let span = Span::new(spread_start, value.span().end);
                entries.push(CfdBlockEntry::Spread(value, span));
            } else {
                let field = self.parse_field()?;
                entries.push(CfdBlockEntry::Field(field));
            }

            self.skip_ws_and_comments();
            if self.eat_char(',') {
                continue;
            }
            if self.peek_char() != Some('}') {
                return Err(self.error("expected `,` or `}` after block entry"));
            }
        }

        Ok(CfdBlock {
            type_marker,
            entries,
            span: Span::new(start, self.pos),
        })
    }

    fn parse_field(&mut self) -> Result<CfdField, CfdSyntaxDiagnostic> {
        let name_start = self.pos;
        let name = self.parse_key("field name")?;
        let name_span = name.span;
        self.skip_ws_and_comments();
        if name.text == "check" && self.peek_char() == Some('{') {
            return Err(CfdSyntaxDiagnostic {
                message: "check blocks are not valid in CFD data files".to_string(),
                span: name_span,
            });
        }
        self.expect_char(':', "field separator `:`")?;
        let value = self.parse_value()?;
        let span = Span::new(name_start, value.span().end);
        Ok(CfdField {
            name: name.text,
            name_span,
            value,
            span,
        })
    }

    fn parse_value(&mut self) -> Result<CfdValue, CfdSyntaxDiagnostic> {
        self.skip_ws_and_comments();
        match self.peek_char() {
            Some('"') => {
                let start = self.pos;
                let s = self.parse_quoted_string()?;
                Ok(CfdValue::QuotedString(s, Span::new(start, self.pos)))
            }
            Some('[') => self.parse_array(),
            Some('@') => Err(self.error("typed references were removed; use `&key`")),
            Some('&') => self.parse_ref_direct(),
            _ => {
                // Could be: null, scalar, or a block (with optional type marker).
                if self.peek_keyword("null") {
                    let start = self.pos;
                    self.eat_keyword("null");
                    return Ok(CfdValue::Null(Span::new(start, self.pos)));
                }
                // Peek ahead: if after a name token there is `{`, it's a block.
                let saved = self.pos;
                let name_start = self.pos;
                if let Ok(token) = self.parse_name("value") {
                    let name_end = self.pos; // capture before skipping whitespace
                    self.skip_ws_and_comments();
                    if self.peek_char() == Some('{') {
                        // Block with explicit type marker.
                        self.pos = saved;
                        let block = self.parse_block()?;
                        return Ok(CfdValue::Block(block));
                    }
                    // Plain scalar — span must not include trailing whitespace.
                    let span = Span::new(name_start, name_end);
                    return Ok(CfdValue::Scalar(token, span));
                }
                // Fallback: try to parse as a block starting with `{`.
                if self.peek_char() == Some('{') {
                    let block = self.parse_block()?;
                    return Ok(CfdValue::Block(block));
                }
                Err(self.error("expected a value"))
            }
        }
    }

    fn parse_array(&mut self) -> Result<CfdValue, CfdSyntaxDiagnostic> {
        let start = self.pos;
        self.expect_char('[', "array `[`")?;
        let mut items = Vec::new();
        loop {
            self.skip_ws_and_comments();
            if self.eat_char(']') {
                break;
            }
            if self.is_eof() {
                return Err(self.error("unterminated array, expected `]`"));
            }
            if self.eat_spread() {
                let spread_start = self.pos - 3;
                let value = self.parse_value()?;
                let span = Span::new(spread_start, value.span().end);
                items.push(CfdValue::Spread(Box::new(value), span));
            } else {
                items.push(self.parse_value()?);
            }
            self.skip_ws_and_comments();
            if self.eat_char(',') {
                self.skip_ws_and_comments();
                // Allow trailing comma.
                if self.peek_char() == Some(']') {
                    self.pos += 1;
                    break;
                }
                continue;
            }
            if self.peek_char() != Some(']') {
                return Err(self.error("expected `,` or `]` after array item"));
            }
        }
        Ok(CfdValue::Array(items, Span::new(start, self.pos)))
    }

    fn parse_ref_direct(&mut self) -> Result<CfdValue, CfdSyntaxDiagnostic> {
        let start = self.pos;
        self.expect_char('&', "`&`")?;
        let key_start = self.pos;
        let key = self.parse_ref_name("reference key")?;
        let key_span = Span::new(key_start, self.pos);
        if matches!(self.peek_char(), Some('.' | '[')) {
            return Err(self.error("reference paths were removed; use `&key`"));
        }
        let span = Span::new(start, self.pos);
        Ok(CfdValue::Ref(CfdRef {
            key: (key, key_span),
            span,
        }))
    }

    fn error(&self, message: impl Into<String>) -> CfdSyntaxDiagnostic {
        CfdSyntaxDiagnostic {
            message: message.into(),
            span: Span::new(self.pos, self.pos),
        }
    }
}

impl CfdBlock {
    fn into_entries_and_fields(self) -> (Vec<CfdBlockEntry>, Vec<CfdField>) {
        let fields = self
            .entries
            .iter()
            .filter_map(|entry| match entry {
                CfdBlockEntry::Field(f) => Some(f.clone()),
                CfdBlockEntry::Spread(_, _) => None,
            })
            .collect();
        (self.entries, fields)
    }
}
