mod tokens;

use super::ast::{
    CfdAst, CfdBitExpr, CfdBitExprKind, CfdBitOp, CfdBlock, CfdField, CfdFieldReference,
    CfdFormatSegment, CfdFormattedString, CfdFunction, CfdNamespaceDecl, CfdRecord, CfdRef,
    CfdUseDecl, CfdValue,
};
use super::{CfdParseOptions, CfdSyntaxDiagnostic};
use crate::limits::{StructuralBudget, StructureKind, TraversalCursor};
use crate::lexical::{is_identifier_continue, is_identifier_start};
use crate::Span;
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
        let mut namespace = None;
        let mut uses = Vec::new();
        let mut records = Vec::new();
        self.skip_ws_and_comments();

        if self.peek_keyword("namespace") {
            match self.parse_namespace_decl() {
                Ok(declaration) => namespace = Some(declaration),
                Err(diagnostic) => {
                    self.diagnostics.push(diagnostic);
                    self.recover_to_next_record();
                }
            }
            self.skip_ws_and_comments();
        }

        while self.peek_keyword("use") {
            match self.parse_use_decl() {
                Ok(declaration) => uses.push(declaration),
                Err(diagnostic) => {
                    self.diagnostics.push(diagnostic);
                    self.recover_to_next_record();
                }
            }
            self.skip_ws_and_comments();
        }

        while !self.is_eof() {
            if self.peek_keyword("namespace") || self.peek_keyword("use") {
                let start = self.pos;
                let keyword = if self.peek_keyword("namespace") {
                    "namespace"
                } else {
                    "use"
                };
                self.diagnostics.push(CfdSyntaxDiagnostic {
                    message: format!("`{keyword}` must appear before all CFD records"),
                    span: Span::new(start, start + keyword.len()),
                });
                self.pos += keyword.len();
                self.recover_to_next_record();
                self.skip_ws_and_comments();
                continue;
            }
            match self.parse_top_level() {
                Ok(new) => records.extend(new),
                Err(diag) => {
                    self.diagnostics.push(diag);
                    self.recover_to_next_record();
                }
            }
            self.skip_ws_and_comments();
        }
        CfdAst {
            namespace,
            uses,
            records,
        }
    }

    fn parse_namespace_decl(&mut self) -> Result<CfdNamespaceDecl, CfdSyntaxDiagnostic> {
        let start = self.pos;
        if !self.eat_keyword("namespace") {
            return Err(self.error("expected `namespace`"));
        }
        let path = self.parse_qualified_path("namespace path", false)?;
        self.expect_char(';', "`;` after namespace declaration")?;
        Ok(CfdNamespaceDecl {
            path: path.text,
            path_span: path.span,
            span: Span::new(start, self.pos),
        })
    }

    fn parse_use_decl(&mut self) -> Result<CfdUseDecl, CfdSyntaxDiagnostic> {
        let start = self.pos;
        if !self.eat_keyword("use") {
            return Err(self.error("expected `use`"));
        }
        let path = self.parse_qualified_path("use path", true)?;
        self.skip_ws_and_comments();
        let alias = if self.eat_keyword("as") {
            let alias = self.parse_name_token("use alias")?;
            if alias.text.contains("::") || !crate::is_cft_identifier(&alias.text) {
                return Err(CfdSyntaxDiagnostic {
                    message: format!("invalid use alias `{}`", alias.text),
                    span: alias.span,
                });
            }
            Some((alias.text, alias.span))
        } else {
            None
        };
        self.expect_char(';', "`;` after use declaration")?;
        Ok(CfdUseDecl {
            path: path.text,
            path_span: path.span,
            alias,
            span: Span::new(start, self.pos),
        })
    }

    fn parse_qualified_path(
        &mut self,
        label: &str,
        require_qualified: bool,
    ) -> Result<Token, CfdSyntaxDiagnostic> {
        let token = self.parse_name_token(label)?;
        let segments = token.text.split("::").collect::<Vec<_>>();
        if (require_qualified && segments.len() < 2)
            || segments
                .iter()
                .any(|segment| !crate::is_cft_identifier(segment))
        {
            return Err(CfdSyntaxDiagnostic {
                message: format!("invalid {label} `{}`", token.text),
                span: token.span,
            });
        }
        Ok(token)
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
                && (is_identifier_start(ch) || ch == '"')
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
                fields: block.fields,
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
            return Err(CfdSyntaxDiagnostic {
                message: "formatted strings use ordinary quotes; remove the `f` prefix".to_string(),
                span: Span::new(self.pos, self.pos + 1),
            });
        }
        if self.peek_keyword("fn") {
            return self.parse_function();
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

    fn parse_function(&mut self) -> Result<CfdValue, CfdSyntaxDiagnostic> {
        let start = self.pos;
        if !self.eat_keyword("fn") {
            return Err(self.error("expected `fn`"));
        }
        self.skip_ws_and_comments();
        self.expect_char('(', "function parameter list `(`")?;
        self.scan_balanced('(', ')', "unterminated function parameter list")?;
        self.skip_ws_and_comments();
        if !self.source[self.pos..].starts_with("->") {
            return Err(self.error("expected `->` after function parameters"));
        }
        self.pos += 2;
        self.skip_ws_and_comments();
        self.scan_function_return_type()?;
        self.skip_ws_and_comments();
        self.expect_char('{', "function body `{`")?;
        let body_start = self.pos;
        self.scan_function_body()?;
        let body_end = self.pos.saturating_sub(1);
        if let Err(error) =
            super::function::validate_function_body(&self.source[body_start..body_end])
        {
            return Err(CfdSyntaxDiagnostic {
                message: error.message,
                span: Span::new(body_start + error.offset, body_start + error.offset + 1),
            });
        }
        let span = Span::new(start, self.pos);
        Ok(CfdValue::Function(CfdFunction {
            source: self.source[start..self.pos].to_string(),
            span,
            body_span: Span::new(body_start, body_end),
        }))
    }

    fn scan_balanced(
        &mut self,
        open: char,
        close: char,
        message: &str,
    ) -> Result<(), CfdSyntaxDiagnostic> {
        let mut depth = 1_usize;
        while let Some(ch) = self.peek_char() {
            self.pos += ch.len_utf8();
            if ch == open {
                depth += 1;
            } else if ch == close {
                depth -= 1;
                if depth == 0 {
                    return Ok(());
                }
            }
        }
        Err(self.error(message))
    }

    fn scan_function_return_type(&mut self) -> Result<(), CfdSyntaxDiagnostic> {
        let mut started = false;
        let mut parens = 0_usize;
        let mut brackets = 0_usize;
        let mut angles = 0_usize;
        while let Some(ch) = self.peek_char() {
            if ch == '{' && parens == 0 && brackets == 0 && angles == 0 {
                if !started {
                    self.pos += ch.len_utf8();
                    self.scan_balanced('{', '}', "unterminated dictionary return type")?;
                    started = true;
                    continue;
                }
                return Ok(());
            }
            self.pos += ch.len_utf8();
            if !ch.is_whitespace() {
                started = true;
            }
            match ch {
                '(' => parens += 1,
                ')' => parens = parens.saturating_sub(1),
                '[' => brackets += 1,
                ']' => brackets = brackets.saturating_sub(1),
                '<' => angles += 1,
                '>' => angles = angles.saturating_sub(1),
                _ => {}
            }
        }
        Err(self.error("expected function body"))
    }

    fn scan_function_body(&mut self) -> Result<(), CfdSyntaxDiagnostic> {
        let mut depth = 1_usize;
        while let Some(ch) = self.peek_char() {
            self.pos += ch.len_utf8();
            match ch {
                '"' => self.scan_function_string()?,
                '#' => {
                    while let Some(comment) = self.peek_char() {
                        self.pos += comment.len_utf8();
                        if comment == '\n' {
                            break;
                        }
                    }
                }
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Ok(());
                    }
                }
                _ => {}
            }
        }
        Err(self.error("unterminated function body"))
    }

    fn scan_function_string(&mut self) -> Result<(), CfdSyntaxDiagnostic> {
        while let Some(ch) = self.peek_char() {
            self.pos += ch.len_utf8();
            if ch == '\\' {
                let Some(escaped) = self.peek_char() else {
                    return Err(self.error("unterminated string escape in function body"));
                };
                self.pos += escaped.len_utf8();
            } else if ch == '"' {
                return Ok(());
            }
        }
        Err(self.error("unterminated string in function body"))
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
            let reference_span = Span::new(
                span.start.saturating_add(1).saturating_add(pos),
                span.start
                    .saturating_add(1)
                    .saturating_add(pos)
                    .saturating_add(relative_end + 1),
            );
            let reference = match parse_field_reference_text(expression, reference_span) {
                Ok(reference) => reference,
                Err(error) if expression.starts_with('&') => return Err(error),
                Err(_) => {
                    let Some(ch) = rest.chars().next() else {
                        break;
                    };
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
        let Some(ch) = rest.chars().next() else {
            break;
        };
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
        || type_name.as_deref().is_some_and(str::is_empty)
        || key.as_deref().is_some_and(str::is_empty)
        || type_name
            .as_deref()
            .is_some_and(|name| !is_qualified_reference_name(name))
        || key.as_deref().is_some_and(|name| !is_reference_name(name))
        || path.iter().any(|name| !is_reference_name(name))
        || expression.chars().any(char::is_whitespace)
    {
        return Err(CfdSyntaxDiagnostic {
            message:
                "formatted string reference must use `field`, `&key.field`, or `&Type::key.field`"
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
        .is_some_and(is_identifier_start)
        && chars.all(is_identifier_continue)
}

fn is_qualified_reference_name(value: &str) -> bool {
    value.split("::").all(is_reference_name)
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
