use super::{negate_u64_to_i64, Parsed, Parser};
use crate::diagnostics::{CftDiagnostics, CftErrorCode};
use crate::limits::StructureKind;
use crate::syntax::ast::{DefaultExpr, DefaultExprKind};
use crate::syntax::lexer::TokenKind;
use crate::syntax::Span;

impl Parser<'_> {
    pub(super) fn parse_default_expr(&mut self) -> Result<Parsed<DefaultExpr>, CftDiagnostics> {
        let token = self.peek().clone();
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
            TokenKind::Ident(_) => self.parse_name_or_enum_default(),
            TokenKind::LBracket => self.parse_array_default(),
            TokenKind::LBrace => self.parse_braced_default(),
            TokenKind::Amp => self.parse_record_reference_default(),
            TokenKind::Minus => self.parse_negative_default(token.span.start),
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
        self.node(StructureKind::DefaultValue, span, [], || DefaultExpr {
            span,
            kind: DefaultExprKind::StaticPath(crate::syntax::ast::QualifiedName {
                segments,
                span,
            }),
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
