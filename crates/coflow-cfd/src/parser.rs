mod tokens;

use crate::ast::{
    CfdAst, CfdBitExpr, CfdBitExprKind, CfdBitOp, CfdBlock, CfdBlockEntry, CfdField,
    CfdFieldReference, CfdFormatSegment, CfdFormattedString, CfdRecord, CfdRef, CfdValue,
};
use crate::{CfdParseOptions, CfdSyntaxDiagnostic, Span};
use coflow_structure::{StructuralBudget, StructureKind, TraversalCursor};
use tokens::Token;

pub(crate) fn parse(source: &str, options: CfdParseOptions) -> (CfdAst, Vec<CfdSyntaxDiagnostic>) {
    let mut p = Parser::new(source, options);
    let ast = p.parse_root();
    (ast, p.diagnostics)
}

struct Parser<'a> {
    source: &'a str,
    pos: usize,
    pub diagnostics: Vec<CfdSyntaxDiagnostic>,
    budget: StructuralBudget,
    open_nesting: u64,
}

impl<'a> Parser<'a> {
    fn new(source: &'a str, options: CfdParseOptions) -> Self {
        Self {
            source,
            pos: 0,
            diagnostics: Vec::new(),
            budget: StructuralBudget::new(options.structural_limits),
            open_nesting: 0,
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

    /// Skip to a record candidate only after malformed nested syntax has
    /// returned to the structural top level.
    fn recover_to_next_record(&mut self) {
        let mut state = RecoveryState::from_prefix(&self.source[..self.pos]);
        let mut at_line_start = false;
        while !self.is_eof() {
            let Some(ch) = self.peek_char() else {
                break;
            };
            if at_line_start && matches!(ch, ' ' | '\t' | '\r') {
                self.pos += ch.len_utf8();
                continue;
            }
            if at_line_start
                && state.is_top_level()
                && (ch.is_alphabetic() || ch == '"' || ch == '_')
            {
                break;
            }
            at_line_start = ch == '\n';
            state.consume(ch, self.source[self.pos..].starts_with("//"));
            self.pos += ch.len_utf8();
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
            let record = CfdRecord {
                key: first.text,
                key_span: first.span,
                group_type: None,
                type_name,
                type_span,
                entries: block.entries,
                span,
            };
            self.charge_node(span)?;
            Ok(vec![record])
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
            let record = CfdRecord {
                key: key.text,
                key_span: key.span,
                group_type: Some((group_token.text.clone(), group_token.span)),
                type_name,
                type_span,
                entries: block.entries,
                span,
            };
            self.charge_node(span)?;
            records.push(record);

            self.skip_ws_and_comments();
            let _ = self.eat_char(',');
        }
        Ok(records)
    }

    fn parse_block(&mut self) -> Result<CfdBlock, CfdSyntaxDiagnostic> {
        self.enter_nesting()?;
        let result = self.parse_block_inner();
        self.open_nesting = self.open_nesting.saturating_sub(1);
        let block = result?;
        self.charge_node(block.span)?;
        Ok(block)
    }

    fn parse_block_inner(&mut self) -> Result<CfdBlock, CfdSyntaxDiagnostic> {
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
                self.charge_node(span)?;
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
        let field = CfdField {
            name: name.text,
            name_span,
            value,
            span,
        };
        self.charge_node(span)?;
        Ok(field)
    }

    fn parse_value(&mut self) -> Result<CfdValue, CfdSyntaxDiagnostic> {
        let value = self.parse_value_inner()?;
        self.charge_node(value.span())?;
        Ok(value)
    }

    fn parse_value_inner(&mut self) -> Result<CfdValue, CfdSyntaxDiagnostic> {
        self.skip_ws_and_comments();
        if self.source[self.pos..].starts_with("f\"") {
            return self.parse_formatted_string();
        }
        match self.peek_char() {
            Some('"') => {
                let start = self.pos;
                let s = self.parse_quoted_string()?;
                let span = Span::new(start, self.pos);
                if let Some(segments) = parse_automatic_format_segments(&s, span)? {
                    Ok(CfdValue::FormattedString(CfdFormattedString {
                        source: self.source[start..self.pos].to_string(),
                        segments,
                        span,
                    }))
                } else {
                    Ok(CfdValue::QuotedString(s, span))
                }
            }
            Some('[') => self.parse_array(),
            Some('@') => Err(self.error("invalid record reference")),
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
                if self.parse_name("value").is_ok() {
                    self.skip_ws_and_comments();
                    if self.peek_char() == Some('{') {
                        // Block with explicit type marker.
                        self.pos = saved;
                        let block = self.parse_block()?;
                        return Ok(CfdValue::Block(block));
                    }
                }
                // Fallback: try to parse as a block starting with `{`.
                if self.peek_char() == Some('{') {
                    let block = self.parse_block()?;
                    return Ok(CfdValue::Block(block));
                }
                self.pos = saved;
                let (expr, is_expression) = self.parse_bit_or_expr()?;
                if is_expression {
                    Ok(CfdValue::BitExpr(expr))
                } else {
                    let CfdBitExprKind::Value(value) = expr.kind else {
                        return Err(self.error("expected a scalar value"));
                    };
                    Ok(CfdValue::Scalar(value, expr.span))
                }
            }
        }
    }

    fn parse_formatted_string(&mut self) -> Result<CfdValue, CfdSyntaxDiagnostic> {
        let start = self.pos;
        self.pos += 2;
        let mut segments = Vec::new();
        let mut text = String::new();
        while let Some(ch) = self.peek_char() {
            if ch == '"' {
                self.pos += 1;
                if !text.is_empty() {
                    segments.push(CfdFormatSegment::Text(std::mem::take(&mut text)));
                }
                return Ok(CfdValue::FormattedString(CfdFormattedString {
                    source: self.source[start..self.pos].to_string(),
                    segments,
                    span: Span::new(start, self.pos),
                }));
            }
            if ch == '\\' {
                let escape_start = self.pos;
                self.pos += 1;
                let Some(escaped) = self.peek_char() else {
                    return Err(CfdSyntaxDiagnostic {
                        message: "unterminated formatted string escape".to_string(),
                        span: Span::new(escape_start, self.pos),
                    });
                };
                self.pos += escaped.len_utf8();
                match escaped {
                    '"' => text.push('"'),
                    '\\' => text.push('\\'),
                    'n' => text.push('\n'),
                    'r' => text.push('\r'),
                    't' => text.push('\t'),
                    other => {
                        return Err(CfdSyntaxDiagnostic {
                            message: format!("unsupported string escape `\\{other}`"),
                            span: Span::new(escape_start, self.pos),
                        });
                    }
                }
                continue;
            }
            if ch == '{' {
                if self.source[self.pos..].starts_with("{{") {
                    text.push('{');
                    self.pos += 2;
                    continue;
                }
                if !text.is_empty() {
                    segments.push(CfdFormatSegment::Text(std::mem::take(&mut text)));
                }
                segments.push(CfdFormatSegment::Reference(self.parse_field_reference()?));
                continue;
            }
            if ch == '}' {
                if self.source[self.pos..].starts_with("}}") {
                    text.push('}');
                    self.pos += 2;
                    continue;
                }
                return Err(CfdSyntaxDiagnostic {
                    message: "literal `}` in a formatted string must be written as `}}`"
                        .to_string(),
                    span: Span::new(self.pos, self.pos + 1),
                });
            }
            self.pos += ch.len_utf8();
            text.push(ch);
        }
        Err(CfdSyntaxDiagnostic {
            message: "unterminated formatted string".to_string(),
            span: Span::new(start, self.pos),
        })
    }

    fn parse_field_reference(&mut self) -> Result<CfdFieldReference, CfdSyntaxDiagnostic> {
        let start = self.pos;
        self.pos += 1;
        let expression_start = self.pos;
        while self.peek_char().is_some_and(|ch| ch != '}') {
            self.pos += self.peek_char().map_or(0, char::len_utf8);
        }
        if self.peek_char() != Some('}') {
            return Err(CfdSyntaxDiagnostic {
                message: "unterminated formatted string reference".to_string(),
                span: Span::new(start, self.pos),
            });
        }
        let expression_end = self.pos;
        self.pos += 1;
        let expression = self.source[expression_start..expression_end].trim();
        parse_field_reference_text(expression, Span::new(start, self.pos))
    }

    fn parse_bit_or_expr(&mut self) -> Result<(CfdBitExpr, bool), CfdSyntaxDiagnostic> {
        self.parse_bit_binary(Self::parse_bit_xor_expr, '|', CfdBitOp::Or)
    }

    fn parse_bit_xor_expr(&mut self) -> Result<(CfdBitExpr, bool), CfdSyntaxDiagnostic> {
        self.parse_bit_binary(Self::parse_bit_and_expr, '^', CfdBitOp::Xor)
    }

    fn parse_bit_and_expr(&mut self) -> Result<(CfdBitExpr, bool), CfdSyntaxDiagnostic> {
        self.parse_bit_binary(Self::parse_bit_primary, '&', CfdBitOp::And)
    }

    fn parse_bit_binary(
        &mut self,
        operand: fn(&mut Self) -> Result<(CfdBitExpr, bool), CfdSyntaxDiagnostic>,
        symbol: char,
        op: CfdBitOp,
    ) -> Result<(CfdBitExpr, bool), CfdSyntaxDiagnostic> {
        let (mut lhs, mut is_expression) = operand(self)?;
        loop {
            self.skip_ws_and_comments();
            if !self.eat_char(symbol) {
                break;
            }
            let (rhs, _) = operand(self)?;
            let span = Span::new(lhs.span.start, rhs.span.end);
            lhs = CfdBitExpr {
                kind: CfdBitExprKind::Binary {
                    op,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                },
                span,
            };
            self.charge_node(span)?;
            is_expression = true;
        }
        Ok((lhs, is_expression))
    }

    fn parse_bit_primary(&mut self) -> Result<(CfdBitExpr, bool), CfdSyntaxDiagnostic> {
        self.skip_ws_and_comments();
        if self.eat_char('(') {
            self.enter_nesting()?;
            let start = self.pos - 1;
            let result = (|| {
                let (mut expr, _) = self.parse_bit_or_expr()?;
                self.expect_char(')', "closing `)`")?;
                expr.span = Span::new(start, self.pos);
                Ok((expr, true))
            })();
            self.open_nesting = self.open_nesting.saturating_sub(1);
            return result;
        }
        let token = self.parse_name_token("flag expression operand")?;
        Ok((
            CfdBitExpr {
                kind: CfdBitExprKind::Value(token.text),
                span: token.span,
            },
            false,
        ))
    }

    fn parse_array(&mut self) -> Result<CfdValue, CfdSyntaxDiagnostic> {
        self.enter_nesting()?;
        let result = self.parse_array_inner();
        self.open_nesting = self.open_nesting.saturating_sub(1);
        result
    }

    fn parse_array_inner(&mut self) -> Result<CfdValue, CfdSyntaxDiagnostic> {
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
                self.charge_node(span)?;
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
            return Err(self.error("invalid record reference"));
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

    fn enter_nesting(&mut self) -> Result<(), CfdSyntaxDiagnostic> {
        let observed = self.open_nesting.saturating_add(1);
        self.budget
            .check_additional_depth(TraversalCursor::root(), StructureKind::SyntaxAst, observed)
            .map_err(|error| self.error(error.to_string()))?;
        self.open_nesting = observed;
        Ok(())
    }

    fn charge_node(&mut self, span: Span) -> Result<(), CfdSyntaxDiagnostic> {
        self.budget
            .charge_nodes(StructureKind::SyntaxAst, 1)
            .map_err(|error| CfdSyntaxDiagnostic {
                message: error.to_string(),
                span,
            })?;
        self.budget
            .charge_work(StructureKind::SyntaxAst, 1)
            .map_err(|error| CfdSyntaxDiagnostic {
                message: error.to_string(),
                span,
            })
    }
}

fn parse_automatic_format_segments(
    value: &str,
    span: Span,
) -> Result<Option<Vec<CfdFormatSegment>>, CfdSyntaxDiagnostic> {
    if !value.contains('{') {
        return Ok(None);
    }

    let mut segments = Vec::new();
    let mut text = String::new();
    let mut pos = 0;
    let mut has_reference = false;
    while pos < value.len() {
        let rest = &value[pos..];
        if rest.starts_with("{{") {
            text.push('{');
            pos += 2;
            continue;
        }
        if rest.starts_with("}}") {
            text.push('}');
            pos += 2;
            continue;
        }
        if rest.starts_with('{') {
            let Some(relative_end) = rest.find('}') else {
                break;
            };
            let expression = rest[1..relative_end].trim();
            let reference = match parse_field_reference_text(expression, span) {
                Ok(reference) => reference,
                Err(error) if expression.starts_with('&') => return Err(error),
                Err(_) => {
                    let ch = rest.chars().next().expect("non-empty string remainder");
                    text.push(ch);
                    pos += ch.len_utf8();
                    continue;
                }
            };
            if !text.is_empty() {
                segments.push(CfdFormatSegment::Text(std::mem::take(&mut text)));
            }
            segments.push(CfdFormatSegment::Reference(reference));
            has_reference = true;
            pos += relative_end + 1;
            continue;
        }
        let ch = rest.chars().next().expect("non-empty string remainder");
        text.push(ch);
        pos += ch.len_utf8();
    }
    if !text.is_empty() {
        segments.push(CfdFormatSegment::Text(text));
    }
    Ok(has_reference.then_some(segments))
}

fn parse_field_reference_text(
    expression: &str,
    span: Span,
) -> Result<CfdFieldReference, CfdSyntaxDiagnostic> {
    let (type_name, key, path) = if let Some(reference) = expression.strip_prefix('&') {
        let (type_name, record) = reference
            .split_once("::")
            .map_or((None, reference), |(type_name, record)| {
                (Some(type_name.to_string()), record)
            });
        let mut parts = record.split('.');
        let key = parts.next().unwrap_or_default();
        let path = parts.map(str::to_string).collect::<Vec<_>>();
        (type_name, Some(key.to_string()), path)
    } else {
        (
            None,
            None,
            expression.split('.').map(str::to_string).collect::<Vec<_>>(),
        )
    };
    if path.is_empty()
        || type_name.as_deref().is_some_and(str::is_empty)
        || key.as_deref().is_some_and(str::is_empty)
        || type_name.as_deref().is_some_and(|name| !is_reference_name(name))
        || key.as_deref().is_some_and(|name| !is_reference_name(name))
        || path.iter().any(|name| !is_reference_name(name))
        || expression.chars().any(char::is_whitespace)
    {
        return Err(CfdSyntaxDiagnostic {
            message: "formatted string reference must use `field`, `&key.field`, or `&Type::key.field`"
                .to_string(),
            span,
        });
    }
    Ok(CfdFieldReference {
        type_name,
        key,
        path,
        span,
    })
}

fn is_reference_name(value: &str) -> bool {
    let mut chars = value.chars();
    chars
        .next()
        .is_some_and(|ch| ch == '_' || ch.is_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_alphanumeric())
}

#[derive(Default)]
struct RecoveryState {
    braces: u64,
    brackets: u64,
    in_string: bool,
    escaped: bool,
    line_comment: bool,
}

impl RecoveryState {
    fn from_prefix(source: &str) -> Self {
        let mut state = Self::default();
        let mut chars = source.chars().peekable();
        while let Some(ch) = chars.next() {
            state.consume(ch, ch == '/' && chars.peek() == Some(&'/'));
        }
        state
    }

    const fn is_top_level(&self) -> bool {
        self.braces == 0 && self.brackets == 0 && !self.in_string && !self.line_comment
    }

    fn consume(&mut self, ch: char, starts_line_comment: bool) {
        if self.line_comment {
            if ch == '\n' {
                self.line_comment = false;
            }
            return;
        }
        if self.in_string {
            if self.escaped {
                self.escaped = false;
            } else if ch == '\\' {
                self.escaped = true;
            } else if ch == '"' {
                self.in_string = false;
            }
            return;
        }
        if starts_line_comment || ch == '#' {
            self.line_comment = true;
            return;
        }
        match ch {
            '"' => self.in_string = true,
            '{' => self.braces = self.braces.saturating_add(1),
            '}' => self.braces = self.braces.saturating_sub(1),
            '[' => self.brackets = self.brackets.saturating_add(1),
            ']' => self.brackets = self.brackets.saturating_sub(1),
            _ => {}
        }
    }
}
