use super::{negate_u64_to_i64, Parsed, Parser};
use crate::diagnostics::{CftDiagnostics, CftErrorCode};
use crate::limits::StructureKind;
use crate::syntax::ast::{DefaultExpr, DefaultExprKind};
use crate::syntax::lexer::TokenKind;
use crate::syntax::Span;

impl Parser<'_> {
    pub(super) fn parse_default_expr(&mut self) -> Result<Parsed<DefaultExpr>, CftDiagnostics> {
        self.parse_default_bit_or()
    }

    fn parse_default_bit_or(&mut self) -> Result<Parsed<DefaultExpr>, CftDiagnostics> {
        let mut lhs = self.parse_default_bit_xor()?;
        while self.eat(&TokenKind::Pipe).is_some() {
            let rhs = self.parse_default_bit_xor()?;
            lhs = self.combine_default_bit_expr(crate::syntax::ast::DefaultBitOp::Or, lhs, rhs)?;
        }
        Ok(lhs)
    }

    fn parse_default_bit_xor(&mut self) -> Result<Parsed<DefaultExpr>, CftDiagnostics> {
        let mut lhs = self.parse_default_bit_and()?;
        while self.eat(&TokenKind::Caret).is_some() {
            let rhs = self.parse_default_bit_and()?;
            lhs = self.combine_default_bit_expr(crate::syntax::ast::DefaultBitOp::Xor, lhs, rhs)?;
        }
        Ok(lhs)
    }

    fn parse_default_bit_and(&mut self) -> Result<Parsed<DefaultExpr>, CftDiagnostics> {
        let mut lhs = self.parse_default_primary()?;
        while self.eat(&TokenKind::Amp).is_some() {
            let rhs = self.parse_default_primary()?;
            lhs = self.combine_default_bit_expr(crate::syntax::ast::DefaultBitOp::And, lhs, rhs)?;
        }
        Ok(lhs)
    }

    fn combine_default_bit_expr(
        &mut self,
        op: crate::syntax::ast::DefaultBitOp,
        lhs: Parsed<DefaultExpr>,
        rhs: Parsed<DefaultExpr>,
    ) -> Result<Parsed<DefaultExpr>, CftDiagnostics> {
        let span = Span::new(lhs.value.span.start, rhs.value.span.end);
        self.node(
            StructureKind::DefaultValue,
            span,
            [lhs.depth, rhs.depth],
            || DefaultExpr {
                kind: DefaultExprKind::BitExpr {
                    op,
                    lhs: Box::new(lhs.value),
                    rhs: Box::new(rhs.value),
                },
                span,
            },
        )
    }

    fn parse_default_primary(&mut self) -> Result<Parsed<DefaultExpr>, CftDiagnostics> {
        let token = self.peek().clone();
        if self.peek_ident_is("fn") {
            return self.parse_function_default();
        }
        match token.kind {
            TokenKind::Int(value) => {
                self.bump();
                self.node(StructureKind::DefaultValue, token.span, [], || {
                    DefaultExpr {
                        kind: DefaultExprKind::Int(value),
                        span: token.span,
                    }
                })
            }
            TokenKind::Float(value) => {
                self.bump();
                self.node(StructureKind::DefaultValue, token.span, [], || {
                    DefaultExpr {
                        kind: DefaultExprKind::Float(value),
                        span: token.span,
                    }
                })
            }
            TokenKind::True => {
                self.bump();
                self.node(StructureKind::DefaultValue, token.span, [], || {
                    DefaultExpr {
                        kind: DefaultExprKind::Bool(true),
                        span: token.span,
                    }
                })
            }
            TokenKind::False => {
                self.bump();
                self.node(StructureKind::DefaultValue, token.span, [], || {
                    DefaultExpr {
                        kind: DefaultExprKind::Bool(false),
                        span: token.span,
                    }
                })
            }
            TokenKind::String(value) => {
                self.bump();
                self.node(StructureKind::DefaultValue, token.span, [], || {
                    DefaultExpr {
                        kind: DefaultExprKind::String(value),
                        span: token.span,
                    }
                })
            }
            TokenKind::FormattedStringStart => self.parse_formatted_string_default(),
            TokenKind::Ident(_) => self.parse_name_or_enum_default(),
            TokenKind::LBracket => self.parse_array_default(),
            TokenKind::LBrace => self.parse_braced_default(),
            TokenKind::Amp => self.parse_record_reference_default(),
            TokenKind::Minus => self.parse_negative_default(token.span.start),
            TokenKind::LParen => {
                let opener = self.expect_simple(&TokenKind::LParen, CftErrorCode::ExpectedToken)?;
                let mut value = self.nested(StructureKind::DefaultValue, opener, |parser| {
                    parser.parse_default_expr()
                })?;
                let end = self
                    .expect_simple(&TokenKind::RParen, CftErrorCode::ExpectedToken)?
                    .end;
                value.value.span = Span::new(opener.start, end);
                Ok(value)
            }
            TokenKind::UIntOverflow(_) => self.err(
                CftErrorCode::InvalidIntLiteral,
                "integer literal out of range",
            ),
            _ => self.err(
                CftErrorCode::InvalidDefaultExpression,
                "expected default expression",
            ),
        }
    }

    fn parse_formatted_string_default(
        &mut self,
    ) -> Result<Parsed<DefaultExpr>, CftDiagnostics> {
        let start = self
            .expect_simple(
                &TokenKind::FormattedStringStart,
                CftErrorCode::ExpectedToken,
            )?
            .start;
        while !self.at(&TokenKind::FormattedStringEnd) {
            if self.at(&TokenKind::Eof) {
                return self.err(CftErrorCode::UnexpectedEof, "unterminated formatted string");
            }
            self.bump();
        }
        let end = self
            .expect_simple(&TokenKind::FormattedStringEnd, CftErrorCode::ExpectedToken)?
            .end;
        let span = Span::new(start, end);
        let source = self.source[start..end].to_string();
        self.node(StructureKind::DefaultValue, span, [], || DefaultExpr {
            kind: DefaultExprKind::FormattedString(source),
            span,
        })
    }

    fn parse_function_default(&mut self) -> Result<Parsed<DefaultExpr>, CftDiagnostics> {
        let start = self.peek().span.start;
        let signature = self.parse_value_type()?;
        if !matches!(
            signature.value.kind,
            crate::syntax::ast::TypeRefKind::Function(_, _)
        ) {
            return self.err(
                CftErrorCode::InvalidDefaultExpression,
                "expected function literal",
            );
        }
        let opener = self.expect_simple(&TokenKind::LBrace, CftErrorCode::ExpectedToken)?;
        let body_start = opener.end;
        let mut depth = 1_usize;
        let mut end = opener.end;
        while depth > 0 {
            let token = self.peek().clone();
            match token.kind {
                TokenKind::LBrace => depth += 1,
                TokenKind::RBrace => depth -= 1,
                TokenKind::Eof => {
                    return self.err(CftErrorCode::UnexpectedEof, "unterminated function body");
                }
                _ => {}
            }
            end = self.bump().span.end;
        }
        let body_end = end.saturating_sub(1);
        if let Err(error) = crate::cfd::validate_function_body(&self.source[body_start..body_end]) {
            return self.err_at(
                CftErrorCode::InvalidDefaultExpression,
                Span::new(
                    body_start + error.offset,
                    (body_start + error.offset + 1).min(body_end),
                ),
                error.message,
            );
        }
        let span = Span::new(start, end);
        let source = self.source[start..end].to_string();
        let signature_depth = signature.depth;
        self.node(
            StructureKind::DefaultValue,
            span,
            [signature_depth],
            || DefaultExpr {
                kind: DefaultExprKind::Function {
                    signature: signature.value,
                    source,
                },
                span,
            },
        )
    }

    fn parse_record_reference_default(
        &mut self,
    ) -> Result<Parsed<DefaultExpr>, CftDiagnostics> {
        let start = self
            .expect_simple(&TokenKind::Amp, CftErrorCode::ExpectedToken)?
            .start;
        let path = self.expect_qualified_name()?;
        if path.segments.len() < 2 {
            return self.err_at(
                CftErrorCode::InvalidDefaultExpression,
                path.span,
                "record reference must contain a type and key separated by `::`",
            );
        }
        let span = Span::new(start, path.span.end);
        self.node(StructureKind::DefaultValue, span, [], || DefaultExpr {
            kind: DefaultExprKind::RecordReference(path),
            span,
        })
    }

    fn parse_negative_default(
        &mut self,
        start: usize,
    ) -> Result<Parsed<DefaultExpr>, CftDiagnostics> {
        self.bump();
        let next = self.peek().clone();
        let span = Span::new(start, next.span.end);
        match next.kind {
            TokenKind::Int(value) => {
                self.bump();
                let Some(negated) = value.checked_neg() else {
                    return self.err_at(
                        CftErrorCode::InvalidIntLiteral,
                        span,
                        "negated integer literal overflowed",
                    );
                };
                self.node(StructureKind::DefaultValue, span, [], || DefaultExpr {
                    kind: DefaultExprKind::Int(negated),
                    span,
                })
            }
            TokenKind::UIntOverflow(value) => {
                self.bump();
                let Some(negated) = negate_u64_to_i64(value) else {
                    return self.err_at(
                        CftErrorCode::InvalidIntLiteral,
                        span,
                        "integer literal out of range",
                    );
                };
                self.node(StructureKind::DefaultValue, span, [], || DefaultExpr {
                    kind: DefaultExprKind::Int(negated),
                    span,
                })
            }
            TokenKind::Float(value) => {
                self.bump();
                self.node(StructureKind::DefaultValue, span, [], || DefaultExpr {
                    kind: DefaultExprKind::Float(-value),
                    span,
                })
            }
            _ => self.err(
                CftErrorCode::InvalidDefaultExpression,
                "expected number after `-`",
            ),
        }
    }

    fn parse_name_or_enum_default(&mut self) -> Result<Parsed<DefaultExpr>, CftDiagnostics> {
        let first = self.expect_ident()?;
        if first.name == "None" {
            return self.node(StructureKind::DefaultValue, first.span, [], || DefaultExpr {
                span: first.span,
                kind: DefaultExprKind::OptionNone,
            });
        }
        if matches!(first.name.as_str(), "Some" | "Ok" | "Err") {
            self.expect_simple(&TokenKind::LParen, CftErrorCode::ExpectedToken)?;
            let inner_span = self.peek().span;
            let inner = self.nested(StructureKind::DefaultValue, inner_span, |parser| {
                parser.parse_default_expr()
            })?;
            let end = self
                .expect_simple(&TokenKind::RParen, CftErrorCode::ExpectedToken)?
                .end;
            let span = Span::new(first.span.start, end);
            let depth = inner.depth;
            return self.node(StructureKind::DefaultValue, first.span, [depth], || DefaultExpr {
                span,
                kind: match first.name.as_str() {
                    "Some" => DefaultExprKind::OptionSome(Box::new(inner.value)),
                    "Ok" => DefaultExprKind::ResultOk(Box::new(inner.value)),
                    "Err" => DefaultExprKind::ResultErr(Box::new(inner.value)),
                    _ => unreachable!(),
                },
            });
        }
        let start = first.span.start;
        let mut end = first.span.end;
        let mut segments = vec![first];
        while self.eat(&TokenKind::DoubleColon).is_some() {
            let segment = self.expect_ident()?;
            end = segment.span.end;
            segments.push(segment);
        }
        let span = Span::new(start, end);
        let path = crate::syntax::ast::QualifiedName { segments, span };
        if self.at(&TokenKind::LBrace) {
            return self.parse_typed_object_default(path);
        }
        self.node(StructureKind::DefaultValue, span, [], || DefaultExpr {
            span,
            kind: DefaultExprKind::StaticPath(path),
        })
    }

    fn parse_typed_object_default(
        &mut self,
        type_name: crate::syntax::ast::QualifiedName,
    ) -> Result<Parsed<DefaultExpr>, CftDiagnostics> {
        let start = type_name.span.start;
        let opener = self.expect_simple(&TokenKind::LBrace, CftErrorCode::ExpectedToken)?;
        let mut fields = Vec::new();
        while !self.at(&TokenKind::RBrace) {
            if self.at(&TokenKind::Eof) {
                return self.err(CftErrorCode::UnexpectedEof, "unterminated object default");
            }
            let name = self.expect_ident()?;
            self.expect_simple(&TokenKind::Colon, CftErrorCode::ExpectedToken)?;
            let value_span = self.peek().span;
            let value = self.nested(StructureKind::DefaultValue, value_span, |parser| {
                parser.parse_default_expr()
            })?;
            fields.push((name, value));
            if self.eat(&TokenKind::Comma).is_none() {
                break;
            }
        }
        let end = self
            .expect_simple(&TokenKind::RBrace, CftErrorCode::ExpectedToken)?
            .end;
        let span = Span::new(start, end);
        let depths = fields
            .iter()
            .map(|(_, value)| value.depth)
            .collect::<Vec<_>>();
        self.node(StructureKind::DefaultValue, opener, depths, || DefaultExpr {
            kind: DefaultExprKind::TypedObject {
                type_name,
                fields: fields
                    .into_iter()
                    .map(|(name, value)| (name, value.value))
                    .collect(),
            },
            span,
        })
    }

    fn parse_array_default(&mut self) -> Result<Parsed<DefaultExpr>, CftDiagnostics> {
        let opener = self.expect_simple(&TokenKind::LBracket, CftErrorCode::ExpectedToken)?;
        let start = opener.start;
        let mut items = Vec::new();
        while !self.at(&TokenKind::RBracket) {
            if self.at(&TokenKind::Eof) {
                return self.err(CftErrorCode::UnexpectedEof, "unterminated array default");
            }
            let child_span = self.peek().span;
            items.push(
                self.nested(StructureKind::DefaultValue, child_span, |parser| {
                    parser.parse_default_expr()
                })?,
            );
            if self.eat(&TokenKind::Comma).is_none() {
                break;
            }
        }
        let end = self
            .expect_simple(&TokenKind::RBracket, CftErrorCode::ExpectedToken)?
            .end;
        let span = Span::new(start, end);
        let depths = items.iter().map(|item| item.depth).collect::<Vec<_>>();
        self.node(StructureKind::DefaultValue, opener, depths, || {
            DefaultExpr {
                kind: DefaultExprKind::Array(items.into_iter().map(|item| item.value).collect()),
                span,
            }
        })
    }

    fn parse_braced_default(&mut self) -> Result<Parsed<DefaultExpr>, CftDiagnostics> {
        let opener = self.expect_simple(&TokenKind::LBrace, CftErrorCode::ExpectedToken)?;
        let start = opener.start;
        if self.at(&TokenKind::RBrace) {
            let end = self.bump().span.end;
            let span = Span::new(start, end);
            return self.node(StructureKind::DefaultValue, opener, [], || DefaultExpr {
                kind: DefaultExprKind::Object(Vec::new()),
                span,
            });
        }
        if !matches!(self.peek().kind, TokenKind::Ident(_))
            || self.next_at(&TokenKind::DoubleColon)
        {
            return self.parse_dictionary_default_after_opener(opener);
        }
        let mut fields = Vec::new();
        while !self.at(&TokenKind::RBrace) {
            if self.at(&TokenKind::Eof) {
                return self.err(CftErrorCode::UnexpectedEof, "unterminated object default");
            }
            let name = self.expect_ident()?;
            self.expect_simple(&TokenKind::Colon, CftErrorCode::ExpectedToken)?;
            let value_span = self.peek().span;
            let value = self.nested(StructureKind::DefaultValue, value_span, |parser| {
                parser.parse_default_expr()
            })?;
            fields.push((name, value));
            if self.eat(&TokenKind::Comma).is_none() {
                break;
            }
        }
        let end = self
            .expect_simple(&TokenKind::RBrace, CftErrorCode::ExpectedToken)?
            .end;
        let span = Span::new(start, end);
        let depths = fields
            .iter()
            .map(|(_, value)| value.depth)
            .collect::<Vec<_>>();
        self.node(StructureKind::DefaultValue, opener, depths, || {
            DefaultExpr {
                kind: DefaultExprKind::Object(
                    fields
                        .into_iter()
                        .map(|(name, value)| (name, value.value))
                        .collect(),
                ),
                span,
            }
        })
    }

    fn parse_dictionary_default_after_opener(
        &mut self,
        opener: Span,
    ) -> Result<Parsed<DefaultExpr>, CftDiagnostics> {
        let start = opener.start;
        let mut entries = Vec::new();
        while !self.at(&TokenKind::RBrace) {
            if self.at(&TokenKind::Eof) {
                return self.err(CftErrorCode::UnexpectedEof, "unterminated dictionary default");
            }
            let key_span = self.peek().span;
            let key = self.nested(StructureKind::DefaultValue, key_span, |parser| {
                parser.parse_default_expr()
            })?;
            self.expect_simple(&TokenKind::Colon, CftErrorCode::ExpectedToken)?;
            let value_span = self.peek().span;
            let value = self.nested(StructureKind::DefaultValue, value_span, |parser| {
                parser.parse_default_expr()
            })?;
            entries.push((key, value));
            if self.eat(&TokenKind::Comma).is_none() {
                break;
            }
        }
        let end = self
            .expect_simple(&TokenKind::RBrace, CftErrorCode::ExpectedToken)?
            .end;
        let span = Span::new(start, end);
        let depths = entries
            .iter()
            .flat_map(|(key, value)| [key.depth, value.depth])
            .collect::<Vec<_>>();
        self.node(StructureKind::DefaultValue, opener, depths, || DefaultExpr {
            kind: DefaultExprKind::Dictionary(
                entries
                    .into_iter()
                    .map(|(key, value)| (key.value, value.value))
                    .collect(),
            ),
            span,
        })
    }
}
