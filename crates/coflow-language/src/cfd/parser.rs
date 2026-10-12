mod tokens;

use super::ast::{
    CfdAst, CfdBitExpr, CfdBitExprKind, CfdBitOp, CfdBlock, CfdDimensionBlock, CfdField,
    CfdFieldReference, CfdFormatSegment, CfdFormattedString, CfdRecord, CfdRef, CfdValue,
};
use super::{CfdParseOptions, CfdSyntaxDiagnostic};
use crate::lexical::{
    decode_simple_escape, is_identifier_continue, is_identifier_start, tokenize_lossless,
    DelimiterNesting, LosslessTokenKind,
};
use crate::limits::{StructuralBudget, StructureKind, TraversalCursor};
use crate::Span;
use tokens::Token;

pub(crate) fn parse(source: &str, options: CfdParseOptions) -> (CfdAst, Vec<CfdSyntaxDiagnostic>) {
    let mut p = Parser::new(source, options);
    let ast = p.parse_root();
    (ast, p.diagnostics)
}

enum TopLevel {
    Records(Vec<CfdRecord>),
    Dimension(CfdDimensionBlock),
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
        let mut dimensions = Vec::new();
        self.skip_ws_and_comments();

        while !self.is_eof() {
            match self.parse_top_level() {
                Ok(TopLevel::Records(new)) => records.extend(new),
                Ok(TopLevel::Dimension(block)) => dimensions.push(block),
                Err(diag) => {
                    self.diagnostics.push(diag);
                    self.recover_to_next_record();
                }
            }
            self.skip_ws_and_comments();
        }
        CfdAst {
            records,
            dimensions,
        }
    }

    /// Skip to a record candidate only after malformed nested syntax has
    /// returned to the structural top level.
    fn recover_to_next_record(&mut self) {
        let mut nesting = DelimiterNesting::default();
        let mut at_line_start = true;
        for token in tokenize_lossless(self.source) {
            if token.span.start >= self.pos
                && at_line_start
                && nesting.is_top_level()
                && matches!(
                    token.kind,
                    LosslessTokenKind::Identifier | LosslessTokenKind::String
                )
            {
                self.pos = token.span.start;
                return;
            }
            nesting.consume(token, self.source);
            at_line_start = match token.kind {
                LosslessTokenKind::Newline => true,
                LosslessTokenKind::Whitespace | LosslessTokenKind::Comment if at_line_start => true,
                _ => false,
            };
        }
        self.pos = self.source.len();
    }

    fn parse_top_level(&mut self) -> Result<TopLevel, CfdSyntaxDiagnostic> {
        let first = self.parse_key("record key or group type")?;
        self.skip_ws_and_comments();

        if self.eat_char(':') {
            // `key: TypeName { ... }`
            self.skip_ws_and_comments();
            let type_start = self.pos;
            let type_name = self.parse_type_name("record type")?;
            let type_span = Span::new(type_start, self.pos);
            let block = self.parse_block()?;
            let span = Span::new(first.span.start, block.span.end);
            let record = CfdRecord {
                key: first.text,
                key_span: first.span,
                group_type: None,
                type_name,
                type_span,
                fields: block.fields,
                span,
            };
            self.charge_node(span)?;
            Ok(TopLevel::Records(vec![record]))
        } else if self.peek_char() == Some('{') {
            // `GroupType { ... }`
            self.parse_group(&first).map(TopLevel::Records)
        } else if self.peek_char() == Some('[') {
            self.parse_dimension_block(first).map(TopLevel::Dimension)
        } else {
            Err(self.error("expected `:`, `[` or `{`"))
        }
    }

    fn parse_dimension_block(
        &mut self,
        dimension: Token,
    ) -> Result<CfdDimensionBlock, CfdSyntaxDiagnostic> {
        // `[` 与 `:` 在词法上区分覆盖和基础记录，不查询任何声明。
        let valid_identifier = |value: &str| {
            let mut chars = value.chars();
            chars.next().is_some_and(is_identifier_start) && chars.all(is_identifier_continue)
        };
        if self.source[dimension.span.start..].starts_with('"')
            || !valid_identifier(&dimension.text)
        {
            return Err(self.error("dimension must be an unquoted identifier"));
        }
        self.expect_char('[', "dimension variant `[`")?;
        let variant = self.parse_name_token("dimension variant")?;
        if !valid_identifier(&variant.text) {
            return Err(self.error("variant must be an identifier"));
        }
        self.skip_ws_and_comments();
        self.expect_char(']', "dimension variant `]`")?;
        self.skip_ws_and_comments();
        self.expect_char('{', "dimension body `{`")?;
        self.enter_nesting()?;
        let result = (|| {
            let mut records = Vec::new();
            loop {
                self.skip_ws_and_comments();
                if self.eat_char('}') {
                    break;
                }
                if self.is_eof() {
                    return Err(self.error("unterminated dimension block"));
                }
                let key = self.parse_key("override record key")?;
                self.skip_ws_and_comments();
                self.expect_char(':', "override record separator `:`")?;
                self.skip_ws_and_comments();
                let type_start = self.pos;
                let type_name = self.parse_type_name("override record type")?;
                let type_span = Span::new(type_start, self.pos);
                let block = self.parse_block()?;
                let span = Span::new(key.span.start, block.span.end);
                self.charge_node(span)?;
                records.push(CfdRecord {
                    key: key.text,
                    key_span: key.span,
                    group_type: None,
                    type_name,
                    type_span,
                    fields: block.fields,
                    span,
                });
                self.skip_ws_and_comments();
                let _ = self.eat_char(',');
            }
            let span = Span::new(dimension.span.start, self.pos);
            self.charge_node(span)?;
            Ok(CfdDimensionBlock {
                dimension: dimension.text,
                dimension_span: dimension.span,
                variant: variant.text,
                variant_span: variant.span,
                records,
                span,
            })
        })();
        self.open_nesting = self.open_nesting.saturating_sub(1);
        result
    }

    fn parse_group(&mut self, group_token: &Token) -> Result<Vec<CfdRecord>, CfdSyntaxDiagnostic> {
        if group_token.text.contains("::") {
            return Err(CfdSyntaxDiagnostic {
                message: "group type must be a single identifier".to_string(),
                span: group_token.span,
            });
        }
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
                let name = self.parse_type_name("record type")?;
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
                fields: block.fields,
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
            let name = self.parse_type_name("block type or `{`")?;
            let name_end = self.pos; // capture before whitespace skip
            self.skip_ws_and_comments();
            if self.peek_char() != Some('{') {
                return Err(self.error("expected `{` after type marker"));
            }
            Some((name, Span::new(ts, name_end)))
        };

        let start = self.pos;
        self.expect_char('{', "block start `{`")?;
        let mut fields = Vec::new();

        loop {
            self.skip_ws_and_comments();
            if self.eat_char('}') {
                break;
            }
            if self.is_eof() {
                return Err(self.error("unterminated block, expected `}`"));
            }

            fields.push(self.parse_field()?);

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
            fields,
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
            let start = self.pos;
            self.pos += 1;
            self.parse_quoted_string()?;
            let span = Span::new(start, self.pos);
            let segments = parse_format_segments(&self.source[start + 2..self.pos - 1], span)?;
            return Ok(CfdValue::FormattedString(CfdFormattedString {
                source: self.source[start..self.pos].to_string(),
                segments,
                span,
            }));
        }
        match self.peek_char() {
            Some('"') => {
                let start = self.pos;
                let value = self.parse_quoted_string()?;
                // 普通字符串不解释花括号；插值必须由 f 前缀显式引入。
                Ok(CfdValue::QuotedString(value, Span::new(start, self.pos)))
            }
            Some('[') => self.parse_array(),
            Some('@') => Err(self.error("invalid record reference")),
            Some('&') => self.parse_ref_direct(),
            _ => {
                if self.peek_keyword("None") {
                    let start = self.pos;
                    self.eat_keyword("None");
                    return Ok(CfdValue::OptionNone(Span::new(start, self.pos)));
                }
                for (name, constructor) in [("Some", 0_u8), ("Ok", 1_u8), ("Err", 2_u8)] {
                    if self.peek_keyword(name) {
                        let start = self.pos;
                        self.eat_keyword(name);
                        self.skip_ws_and_comments();
                        self.expect_char('(', "constructor argument `(`")?;
                        let value = self.parse_value()?;
                        self.skip_ws_and_comments();
                        self.expect_char(')', "constructor end `)`")?;
                        let span = Span::new(start, self.pos);
                        return Ok(match constructor {
                            0 => CfdValue::OptionSome(Box::new(value), span),
                            1 => CfdValue::ResultOk(Box::new(value), span),
                            _ => CfdValue::ResultErr(Box::new(value), span),
                        });
                    }
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
        let token = self.parse_value_token("flag expression operand")?;
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
            items.push(self.parse_value()?);
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
        let reference_start = self.pos;
        let reference = self.parse_ref_name("reference key")?;
        let reference_span = Span::new(reference_start, self.pos);
        if matches!(self.peek_char(), Some('.' | '[')) {
            return Err(self.error("invalid record reference"));
        }
        let (type_name, key) = if let Some((type_name, key)) = reference.rsplit_once("::") {
            if type_name.contains("::") {
                return Err(CfdSyntaxDiagnostic {
                    message: "record reference must use `&Type::key`".to_string(),
                    span: reference_span,
                });
            }
            let type_end = reference_start + type_name.len();
            let key_start = type_end + 2;
            (
                Some((type_name.to_string(), Span::new(reference_start, type_end))),
                (key.to_string(), Span::new(key_start, self.pos)),
            )
        } else {
            (None, (reference, reference_span))
        };
        let span = Span::new(start, self.pos);
        Ok(CfdValue::Ref(CfdRef {
            type_name,
            key,
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
        Ok(())
    }
}

fn parse_format_segments(
    value: &str,
    span: Span,
) -> Result<Vec<CfdFormatSegment>, CfdSyntaxDiagnostic> {
    let mut segments = Vec::new();
    let mut text = String::new();
    let mut pos = 0;
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
            let end = rest.find('}').ok_or_else(|| CfdSyntaxDiagnostic {
                message: "unterminated interpolation".to_string(),
                span,
            })?;
            // 在原始字面量上解析路径，转义与 Unicode 都不改变语义高亮和诊断坐标。
            let reference_span = Span::new(span.start + 2 + pos, span.start + 2 + pos + end + 1);
            let mut reference = parse_field_reference_text(rest[1..end].trim(), reference_span)?;
            if reference.key.is_none() {
                if reference.path.len() < 2 || reference.path[0] != "self" {
                    return Err(CfdSyntaxDiagnostic {
                        message: "interpolation requires `self.field` or `&Type::key.field`"
                            .to_string(),
                        span,
                    });
                }
                reference.path.remove(0);
            }
            if !text.is_empty() {
                segments.push(CfdFormatSegment::Text(std::mem::take(&mut text)));
            }
            segments.push(CfdFormatSegment::Reference(reference));
            pos += end + 1;
            continue;
        }
        if rest.starts_with('}') {
            return Err(CfdSyntaxDiagnostic {
                message: "unescaped closing brace".to_string(),
                span,
            });
        }
        if let Some(ch) = rest.chars().next() {
            if ch == '\\' {
                let escaped = rest[1..]
                    .chars()
                    .next()
                    .and_then(decode_simple_escape)
                    .ok_or_else(|| CfdSyntaxDiagnostic {
                        message: "invalid template escape".to_string(),
                        span,
                    })?;
                text.push(escaped);
                pos += 2;
            } else {
                text.push(ch);
                pos += ch.len_utf8();
            }
        }
    }
    if !text.is_empty() {
        segments.push(CfdFormatSegment::Text(text));
    }
    Ok(segments)
}

fn parse_field_reference_text(
    expression: &str,
    span: Span,
) -> Result<CfdFieldReference, CfdSyntaxDiagnostic> {
    let (type_name, key, path) = expression.strip_prefix('&').map_or_else(
        || {
            (
                None,
                None,
                expression
                    .split('.')
                    .map(str::to_string)
                    .collect::<Vec<_>>(),
            )
        },
        |reference| {
            let (type_name, record) = reference
                .rsplit_once("::")
                .map_or((None, reference), |(type_name, record)| {
                    (Some(type_name.to_string()), record)
                });
            let mut parts = record.split('.');
            let key = parts.next().unwrap_or_default();
            let path = parts.map(str::to_string).collect::<Vec<_>>();
            (type_name, Some(key.to_string()), path)
        },
    );
    if path.is_empty()
        || path.len() > 128
        || (key.is_some() && type_name.is_none())
        || type_name.as_deref().is_some_and(str::is_empty)
        || key.as_deref().is_some_and(str::is_empty)
        || type_name
            .as_deref()
            .is_some_and(|name| !is_reference_name(name))
        || key.as_deref().is_some_and(|name| !is_reference_name(name))
        || path.iter().any(|name| !is_reference_name(name))
        || expression.chars().any(char::is_whitespace)
    {
        return Err(CfdSyntaxDiagnostic {
            message: "interpolation requires `self.field` or `&Type::key.field`".to_string(),
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
    chars.next().is_some_and(is_identifier_start) && chars.all(is_identifier_continue)
}
