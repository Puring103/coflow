use super::Parser;
use crate::ast::{CheckExpr, CheckExprKind, NameRef, TypePredicate};
use crate::error::{CftDiagnostics, CftErrorCode};
use crate::lexer::TokenKind;
use crate::span::Span;

impl<'a> Parser<'a> {
    pub(super) fn parse_postfix_expr(&mut self) -> Result<CheckExpr, CftDiagnostics> {
        let mut expr = self.parse_primary_expr()?;
        loop {
            if self.eat(&TokenKind::LParen).is_some() {
                let CheckExprKind::Name(name) = expr.kind else {
                    return self.err(
                        CftErrorCode::UnexpectedToken,
                        "only named functions can be called",
                    );
                };
                let call_name = NameRef {
                    name,
                    span: expr.span,
                };
                let mut args = Vec::new();
                while !self.at(&TokenKind::RParen) {
                    if self.at(&TokenKind::Eof) {
                        return self.err(CftErrorCode::UnexpectedEof, "unterminated function call");
                    }
                    args.push(self.parse_or_expr()?);
                    if self.eat(&TokenKind::Comma).is_none() {
                        break;
                    }
                }
                let end = self
                    .expect_simple(&TokenKind::RParen, CftErrorCode::ExpectedToken)?
                    .end;
                expr = CheckExpr {
                    span: Span::new(call_name.span.start, end),
                    kind: CheckExprKind::Call {
                        name: call_name,
                        args,
                    },
                };
            } else if self.eat(&TokenKind::Dot).is_some() {
                let name = self.expect_ident()?;
                if self.eat(&TokenKind::LParen).is_some() {
                    let mut args = Vec::new();
                    if !self.at(&TokenKind::RParen) {
                        loop {
                            args.push(self.parse_or_expr()?);
                            if self.eat(&TokenKind::Comma).is_none() {
                                break;
                            }
                        }
                    }
                    let end = self
                        .expect_simple(&TokenKind::RParen, CftErrorCode::ExpectedToken)?
                        .end;
                    let span = Span::new(expr.span.start, end);
                    expr = CheckExpr {
                        span,
                        kind: CheckExprKind::MethodCall {
                            receiver: Box::new(expr),
                            name,
                            args,
                        },
                    };
                } else {
                    let span = Span::new(expr.span.start, name.span.end);
                    expr = CheckExpr {
                        span,
                        kind: CheckExprKind::Field {
                            expr: Box::new(expr),
                            name,
                        },
                    };
                }
            } else if self.eat(&TokenKind::LBracket).is_some() {
                let index = self.parse_or_expr()?;
                let end = self
                    .expect_simple(&TokenKind::RBracket, CftErrorCode::ExpectedToken)?
                    .end;
                expr = CheckExpr {
                    span: Span::new(expr.span.start, end),
                    kind: CheckExprKind::Index {
                        expr: Box::new(expr),
                        index: Box::new(index),
                    },
                };
            } else {
                break;
            }
        }
        Ok(expr)
    }

    fn parse_primary_expr(&mut self) -> Result<CheckExpr, CftDiagnostics> {
        let token = self.peek().clone();
        match token.kind {
            TokenKind::Int(value) => {
                self.bump();
                Ok(CheckExpr {
                    kind: CheckExprKind::Int(value),
                    span: token.span,
                })
            }
            TokenKind::Float(value) => {
                self.bump();
                Ok(CheckExpr {
                    kind: CheckExprKind::Float(value),
                    span: token.span,
                })
            }
            TokenKind::True => {
                self.bump();
                Ok(CheckExpr {
                    kind: CheckExprKind::Bool(true),
                    span: token.span,
                })
            }
            TokenKind::False => {
                self.bump();
                Ok(CheckExpr {
                    kind: CheckExprKind::Bool(false),
                    span: token.span,
                })
            }
            TokenKind::Null => {
                self.bump();
                Ok(CheckExpr {
                    kind: CheckExprKind::Null,
                    span: token.span,
                })
            }
            TokenKind::String(value) => {
                self.bump();
                Ok(CheckExpr {
                    kind: CheckExprKind::String(value),
                    span: token.span,
                })
            }
            TokenKind::Ident(value) => {
                self.bump();
                Ok(CheckExpr {
                    kind: CheckExprKind::Name(value),
                    span: token.span,
                })
            }
            TokenKind::LParen => {
                let start = self
                    .expect_simple(&TokenKind::LParen, CftErrorCode::ExpectedToken)?
                    .start;
                let mut expr = self.parse_or_expr()?;
                let end = self
                    .expect_simple(&TokenKind::RParen, CftErrorCode::ExpectedToken)?
                    .end;
                expr.span = Span::new(start, end);
                Ok(expr)
            }
            TokenKind::UIntOverflow(_) => self.err(
                CftErrorCode::InvalidIntLiteral,
                "integer literal out of range",
            ),
            _ => self.err(
                CftErrorCode::InvalidCheckStatement,
                "expected check expression",
            ),
        }
    }

    pub(super) fn parse_type_predicate(&mut self) -> Result<TypePredicate, CftDiagnostics> {
        if let Some(span) = self.eat(&TokenKind::Null) {
            return Ok(TypePredicate::Null(span));
        }
        self.expect_ident().map(TypePredicate::Type)
    }
}
