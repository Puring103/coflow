use super::{negate_u64_to_i64, Parser};
use crate::ast::{DefaultExpr, DefaultExprKind};
use crate::error::{CftDiagnostics, CftErrorCode};
use crate::lexer::TokenKind;
use crate::span::Span;

impl<'a> Parser<'a> {
    pub(super) fn parse_default_expr(&mut self) -> Result<DefaultExpr, CftDiagnostics> {
        let token = self.peek().clone();
        match token.kind {
            TokenKind::Int(value) => {
                self.bump();
                Ok(DefaultExpr {
                    kind: DefaultExprKind::Int(value),
                    span: token.span,
                })
            }
            TokenKind::Float(value) => {
                self.bump();
                Ok(DefaultExpr {
                    kind: DefaultExprKind::Float(value),
                    span: token.span,
                })
            }
            TokenKind::True => {
                self.bump();
                Ok(DefaultExpr {
                    kind: DefaultExprKind::Bool(true),
                    span: token.span,
                })
            }
            TokenKind::False => {
                self.bump();
                Ok(DefaultExpr {
                    kind: DefaultExprKind::Bool(false),
                    span: token.span,
                })
            }
            TokenKind::Null => {
                self.bump();
                Ok(DefaultExpr {
                    kind: DefaultExprKind::Null,
                    span: token.span,
                })
            }
            TokenKind::String(value) => {
                self.bump();
                Ok(DefaultExpr {
                    kind: DefaultExprKind::String(value),
                    span: token.span,
                })
            }
            TokenKind::Ident(_) => self.parse_name_or_enum_default(),
            TokenKind::LBracket => self.parse_array_default(),
            TokenKind::LBrace => self.parse_object_default(),
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

    fn parse_negative_default(&mut self, start: usize) -> Result<DefaultExpr, CftDiagnostics> {
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
                Ok(DefaultExpr {
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
                Ok(DefaultExpr {
                    kind: DefaultExprKind::Int(negated),
                    span,
                })
            }
            TokenKind::Float(value) => {
                self.bump();
                Ok(DefaultExpr {
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

    fn parse_name_or_enum_default(&mut self) -> Result<DefaultExpr, CftDiagnostics> {
        let first = self.expect_ident()?;
        if self.eat(&TokenKind::Dot).is_some() {
            let variant = self.expect_ident()?;
            Ok(DefaultExpr {
                span: first.span.join(variant.span),
                kind: DefaultExprKind::EnumVariant {
                    enum_name: first,
                    variant,
                },
            })
        } else {
            Ok(DefaultExpr {
                span: first.span,
                kind: DefaultExprKind::Name(first),
            })
        }
    }

    fn parse_array_default(&mut self) -> Result<DefaultExpr, CftDiagnostics> {
        let start = self
            .expect_simple(&TokenKind::LBracket, CftErrorCode::ExpectedToken)?
            .start;
        let mut items = Vec::new();
        while !self.at(&TokenKind::RBracket) {
            if self.at(&TokenKind::Eof) {
                return self.err(CftErrorCode::UnexpectedEof, "unterminated array default");
            }
            items.push(self.parse_default_expr()?);
            if self.eat(&TokenKind::Comma).is_none() {
                break;
            }
        }
        let end = self
            .expect_simple(&TokenKind::RBracket, CftErrorCode::ExpectedToken)?
            .end;
        Ok(DefaultExpr {
            kind: DefaultExprKind::Array(items),
            span: Span::new(start, end),
        })
    }

    fn parse_object_default(&mut self) -> Result<DefaultExpr, CftDiagnostics> {
        let start = self
            .expect_simple(&TokenKind::LBrace, CftErrorCode::ExpectedToken)?
            .start;
        let mut fields = Vec::new();
        while !self.at(&TokenKind::RBrace) {
            if self.at(&TokenKind::Eof) {
                return self.err(CftErrorCode::UnexpectedEof, "unterminated object default");
            }
            let name = self.expect_ident()?;
            self.expect_simple(&TokenKind::Colon, CftErrorCode::ExpectedToken)?;
            let value = self.parse_default_expr()?;
            fields.push((name, value));
            if self.eat(&TokenKind::Comma).is_none() {
                break;
            }
        }
        let end = self
            .expect_simple(&TokenKind::RBrace, CftErrorCode::ExpectedToken)?
            .end;
        Ok(DefaultExpr {
            kind: DefaultExprKind::Object(fields),
            span: Span::new(start, end),
        })
    }
}
