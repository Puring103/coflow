use crate::ast::{
    CfdAst, CfdBlock, CfdBlockEntry, CfdField, CfdPathSeg, CfdRecord, CfdRef, CfdRefKind, CfdValue,
};
use crate::{CfdSyntaxDiagnostic};
use coflow_cft::Span;

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
            Ok(vec![CfdRecord {
                key: first.text,
                key_span: first.span,
                type_name,
                type_span,
                fields: block_to_fields(block),
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
            records.push(CfdRecord {
                key: key.text,
                key_span: key.span,
                type_name,
                type_span,
                fields: block_to_fields(block),
                span,
            });
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
            // Allow `,` or `;` as separators, but don't require them.
            self.eat_char(',');
            self.eat_char(';');
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
            Some('@') => self.parse_ref_typed(),
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
            }
        }
        Ok(CfdValue::Array(items, Span::new(start, self.pos)))
    }

    fn parse_ref_typed(&mut self) -> Result<CfdValue, CfdSyntaxDiagnostic> {
        let start = self.pos;
        self.expect_char('@', "`@`")?;
        let type_start = self.pos;
        let type_name = self.parse_ref_name("reference type")?;
        let type_span = Span::new(type_start, self.pos);
        self.expect_char('.', "`.` after reference type")?;
        let key_start = self.pos;
        let key = self.parse_ref_name("reference key")?;
        let key_span = Span::new(key_start, self.pos);
        let path = self.parse_ref_path()?;
        let span = Span::new(start, self.pos);
        Ok(CfdValue::Ref(CfdRef {
            kind: CfdRefKind::Typed,
            type_name: Some((type_name, type_span)),
            key: (key, key_span),
            path,
            span,
        }))
    }

    fn parse_ref_direct(&mut self) -> Result<CfdValue, CfdSyntaxDiagnostic> {
        let start = self.pos;
        self.expect_char('&', "`&`")?;
        let key_start = self.pos;
        let key = self.parse_ref_name("reference key")?;
        let key_span = Span::new(key_start, self.pos);
        let span = Span::new(start, self.pos);
        Ok(CfdValue::Ref(CfdRef {
            kind: CfdRefKind::Direct,
            type_name: None,
            key: (key, key_span),
            path: Vec::new(),
            span,
        }))
    }

    fn parse_ref_path(&mut self) -> Result<Vec<CfdPathSeg>, CfdSyntaxDiagnostic> {
        let mut segs = Vec::new();
        loop {
            self.skip_ws_and_comments();
            if self.eat_char('.') {
                let start = self.pos;
                let name = self.parse_ref_name("path field")?;
                segs.push(CfdPathSeg::Field(name, Span::new(start, self.pos)));
            } else if self.eat_char('[') {
                let start = self.pos;
                let index = self.parse_ref_index()?;
                self.expect_char(']', "closing `]`")?;
                segs.push(CfdPathSeg::Index(index, Span::new(start, self.pos)));
            } else {
                break;
            }
        }
        Ok(segs)
    }

    fn parse_ref_index(&mut self) -> Result<String, CfdSyntaxDiagnostic> {
        self.skip_ws_and_comments();
        if self.peek_char() == Some('"') {
            return self.parse_quoted_string();
        }
        let start = self.pos;
        while let Some(ch) = self.peek_char() {
            if ch == ']' {
                break;
            }
            self.pos += ch.len_utf8();
        }
        let raw = self.source[start..self.pos].trim();
        if raw.is_empty() {
            return Err(self.error("empty reference index"));
        }
        Ok(raw.to_string())
    }

    // ── Token helpers ──────────────────────────────────────────────────────

    fn parse_key(&mut self, label: &str) -> Result<Token, CfdSyntaxDiagnostic> {
        self.skip_ws_and_comments();
        if self.peek_char() == Some('"') {
            let start = self.pos;
            let s = self.parse_quoted_string()?;
            return Ok(Token { text: s, span: Span::new(start, self.pos) });
        }
        self.parse_name_token(label)
    }

    fn parse_name(&mut self, label: &str) -> Result<String, CfdSyntaxDiagnostic> {
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

    fn parse_ref_name(&mut self, label: &str) -> Result<String, CfdSyntaxDiagnostic> {
        self.skip_ws_and_comments();
        let start = self.pos;
        while let Some(ch) = self.peek_char() {
            if ch.is_whitespace()
                || matches!(ch, '.' | '[' | ']' | ',' | ';' | '}' | ')' | ':' | '@' | '&')
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

    fn parse_quoted_string(&mut self) -> Result<String, CfdSyntaxDiagnostic> {
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

    fn skip_ws_and_comments(&mut self) {
        loop {
            while self.peek_char().is_some_and(char::is_whitespace) {
                self.pos += self.peek_char().map_or(0, char::len_utf8);
            }
            if self.source[self.pos..].starts_with("//") {
                self.pos += 2;
                while self.peek_char().is_some_and(|ch| ch != '\n') {
                    self.pos += self.peek_char().map_or(0, char::len_utf8);
                }
                continue;
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

    fn expect_char(&mut self, expected: char, label: &str) -> Result<(), CfdSyntaxDiagnostic> {
        self.skip_ws_and_comments();
        if self.eat_char(expected) {
            Ok(())
        } else {
            Err(self.error(format!("expected {label}")))
        }
    }

    fn eat_char(&mut self, expected: char) -> bool {
        if self.peek_char() == Some(expected) {
            self.pos += expected.len_utf8();
            true
        } else {
            false
        }
    }

    fn eat_spread(&mut self) -> bool {
        self.skip_ws_and_comments();
        if self.source[self.pos..].starts_with("...") {
            self.pos += 3;
            true
        } else {
            false
        }
    }

    fn eat_keyword(&mut self, kw: &str) -> bool {
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

    fn peek_keyword(&self, kw: &str) -> bool {
        if !self.source[self.pos..].starts_with(kw) {
            return false;
        }
        let end = self.pos + kw.len();
        self.source
            .get(end..)
            .and_then(|rest| rest.chars().next())
            .is_none_or(is_value_boundary)
    }

    fn peek_char(&self) -> Option<char> {
        self.source[self.pos..].chars().next()
    }

    fn is_eof(&self) -> bool {
        self.pos >= self.source.len()
    }

    fn error(&self, message: impl Into<String>) -> CfdSyntaxDiagnostic {
        CfdSyntaxDiagnostic {
            message: message.into(),
            span: Span::new(self.pos, self.pos),
        }
    }
}

fn is_value_boundary(ch: char) -> bool {
    ch.is_whitespace() || matches!(ch, ',' | ';' | '}' | ']' | '|' | ':')
}

fn block_to_fields(block: CfdBlock) -> Vec<CfdField> {
    block
        .entries
        .into_iter()
        .filter_map(|entry| match entry {
            CfdBlockEntry::Field(f) => Some(f),
            CfdBlockEntry::Spread(_, _) => None,
        })
        .collect()
}

struct Token {
    text: String,
    span: Span,
}
