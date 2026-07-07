use super::{negate_u64_to_i64, Parser};
use crate::ast::{
    BinOp, CheckBlock, CheckExpr, CheckExprKind, CheckStmt, CmpOp, NameRef, QuantifierKind,
    TypePredicate, UnaryOp,
};
use crate::container::ModuleId;
use crate::error::{CftDiagnostic, CftDiagnostics, CftErrorCode};
use crate::lexer::TokenKind;
use crate::span::Span;

impl<'a> Parser<'a> {
    pub(super) fn parse_check_block(&mut self) -> Result<CheckBlock, CftDiagnostics> {
        let start = self
            .expect_simple(&TokenKind::Check, CftErrorCode::UnexpectedToken)?
            .start;
        self.expect_simple(&TokenKind::LBrace, CftErrorCode::ExpectedToken)?;
        let stmts = self.parse_check_stmts()?;
        let end = self
            .expect_simple(&TokenKind::RBrace, CftErrorCode::ExpectedToken)?
            .end;
        Ok(CheckBlock {
            stmts,
            span: Span::new(start, end),
        })
    }

    fn parse_check_stmts(&mut self) -> Result<Vec<CheckStmt>, CftDiagnostics> {
        let mut stmts = Vec::new();
        while !self.at(&TokenKind::RBrace) {
            if self.at(&TokenKind::Eof) {
                return self.err(CftErrorCode::UnexpectedEof, "unterminated check block");
            }
            stmts.push(self.parse_check_stmt()?);
        }
        Ok(stmts)
    }

    fn parse_check_stmt(&mut self) -> Result<CheckStmt, CftDiagnostics> {
        if let Some(kind) = self.peek_quantifier() {
            return self.parse_quantifier_stmt(kind);
        }
        if self.at(&TokenKind::When) {
            return self.parse_when_stmt();
        }
        let expr = self.parse_or_expr()?;
        if self.eat(&TokenKind::Semicolon).is_none() {
            return self.err(
                CftErrorCode::InvalidCheckStatement,
                "check expression statements must end with `;`",
            );
        }
        Ok(CheckStmt::Expr(expr))
    }

    fn parse_quantifier_stmt(&mut self, kind: QuantifierKind) -> Result<CheckStmt, CftDiagnostics> {
        let start = self.bump().span.start;
        let binding = self.expect_ident()?;
        self.expect_simple(&TokenKind::In, CftErrorCode::ExpectedToken)?;
        let collection = self.parse_or_expr()?;
        self.expect_simple(&TokenKind::LBrace, CftErrorCode::ExpectedToken)?;
        let body = self.parse_check_stmts()?;
        let end = self
            .expect_simple(&TokenKind::RBrace, CftErrorCode::ExpectedToken)?
            .end;
        Ok(CheckStmt::Quantifier {
            kind,
            binding,
            collection,
            body,
            span: Span::new(start, end),
        })
    }

    fn parse_when_stmt(&mut self) -> Result<CheckStmt, CftDiagnostics> {
        let start = self
            .expect_simple(&TokenKind::When, CftErrorCode::UnexpectedToken)?
            .start;
        let condition = self.parse_or_expr()?;
        self.expect_simple(&TokenKind::LBrace, CftErrorCode::ExpectedToken)?;
        let body = self.parse_check_stmts()?;
        let end = self
            .expect_simple(&TokenKind::RBrace, CftErrorCode::ExpectedToken)?
            .end;
        Ok(CheckStmt::When {
            condition,
            body,
            span: Span::new(start, end),
        })
    }

    fn parse_or_expr(&mut self) -> Result<CheckExpr, CftDiagnostics> {
        let mut expr = self.parse_and_expr()?;
        while self.eat(&TokenKind::PipePipe).is_some() {
            let rhs = self.parse_and_expr()?;
            expr = bin_expr(BinOp::Or, expr, rhs);
        }
        Ok(expr)
    }

    fn parse_and_expr(&mut self) -> Result<CheckExpr, CftDiagnostics> {
        let mut expr = self.parse_is_expr()?;
        while self.eat(&TokenKind::AmpAmp).is_some() {
            let rhs = self.parse_is_expr()?;
            expr = bin_expr(BinOp::And, expr, rhs);
        }
        Ok(expr)
    }

    fn parse_is_expr(&mut self) -> Result<CheckExpr, CftDiagnostics> {
        let mut expr = self.parse_cmp_chain()?;
        while self.eat(&TokenKind::Is).is_some() {
            let predicate = self.parse_type_predicate()?;
            let end = match &predicate {
                TypePredicate::Type(name) => name.span.end,
                TypePredicate::Null(span) => span.end,
            };
            expr = CheckExpr {
                span: Span::new(expr.span.start, end),
                kind: CheckExprKind::Is {
                    expr: Box::new(expr),
                    predicate,
                },
            };
        }
        Ok(expr)
    }

    fn parse_cmp_chain(&mut self) -> Result<CheckExpr, CftDiagnostics> {
        let first = self.parse_bitor_expr()?;
        let mut rest = Vec::new();
        while let Some(op) = self.eat_cmp_op() {
            rest.push((op, self.parse_bitor_expr()?));
        }
        if rest.is_empty() {
            return Ok(first);
        }
        validate_cmp_chain(self.module, first.span, &rest)?;
        let end = rest
            .last()
            .map_or(first.span.end, |(_, expr)| expr.span.end);
        Ok(CheckExpr {
            span: Span::new(first.span.start, end),
            kind: CheckExprKind::CmpChain {
                first: Box::new(first),
                rest,
            },
        })
    }

    fn parse_bitor_expr(&mut self) -> Result<CheckExpr, CftDiagnostics> {
        let mut expr = self.parse_add_expr()?;
        loop {
            let op = if self.eat(&TokenKind::Pipe).is_some() {
                BinOp::BitOr
            } else if self.eat(&TokenKind::Caret).is_some() {
                BinOp::BitXor
            } else if self.eat(&TokenKind::Amp).is_some() {
                BinOp::BitAnd
            } else {
                break;
            };
            let rhs = self.parse_add_expr()?;
            expr = bin_expr(op, expr, rhs);
        }
        Ok(expr)
    }

    fn parse_add_expr(&mut self) -> Result<CheckExpr, CftDiagnostics> {
        let mut expr = self.parse_mul_expr()?;
        loop {
            let op = if self.eat(&TokenKind::Plus).is_some() {
                BinOp::Add
            } else if self.eat(&TokenKind::Minus).is_some() {
                BinOp::Sub
            } else if self.eat(&TokenKind::LessLess).is_some() {
                BinOp::Shl
            } else if self.eat(&TokenKind::GreaterGreater).is_some() {
                BinOp::Shr
            } else {
                break;
            };
            let rhs = self.parse_mul_expr()?;
            expr = bin_expr(op, expr, rhs);
        }
        Ok(expr)
    }

    fn parse_mul_expr(&mut self) -> Result<CheckExpr, CftDiagnostics> {
        let mut expr = self.parse_power_expr()?;
        loop {
            let op = if self.eat(&TokenKind::Star).is_some() {
                BinOp::Mul
            } else if self.eat(&TokenKind::Slash).is_some() {
                BinOp::Div
            } else if self.eat(&TokenKind::SlashSlash).is_some() {
                BinOp::IntDiv
            } else if self.eat(&TokenKind::Percent).is_some() {
                BinOp::Mod
            } else {
                break;
            };
            let rhs = self.parse_power_expr()?;
            expr = bin_expr(op, expr, rhs);
        }
        Ok(expr)
    }

    fn parse_power_expr(&mut self) -> Result<CheckExpr, CftDiagnostics> {
        let lhs = self.parse_prefix_expr()?;
        if self.eat(&TokenKind::StarStar).is_some() {
            let rhs = self.parse_power_expr()?;
            Ok(bin_expr(BinOp::Pow, lhs, rhs))
        } else {
            Ok(lhs)
        }
    }

    fn parse_prefix_expr(&mut self) -> Result<CheckExpr, CftDiagnostics> {
        let token = self.peek().clone();
        let op = if self.eat(&TokenKind::Bang).is_some() {
            Some(UnaryOp::Not)
        } else if self.eat(&TokenKind::Tilde).is_some() {
            Some(UnaryOp::BitNot)
        } else if self.eat(&TokenKind::Minus).is_some() {
            Some(UnaryOp::Neg)
        } else {
            None
        };
        if let Some(op) = op {
            if matches!(op, UnaryOp::Neg) {
                if let TokenKind::UIntOverflow(value) = self.peek().kind {
                    let value_token = self.bump();
                    let span = Span::new(token.span.start, value_token.span.end);
                    let Some(negated) = negate_u64_to_i64(value) else {
                        return self.err_at(
                            CftErrorCode::InvalidIntLiteral,
                            span,
                            "integer literal out of range",
                        );
                    };
                    return Ok(CheckExpr {
                        kind: CheckExprKind::Int(negated),
                        span,
                    });
                }
            }
            let expr = self.parse_prefix_expr()?;
            return Ok(CheckExpr {
                span: Span::new(token.span.start, expr.span.end),
                kind: CheckExprKind::Unary {
                    op,
                    expr: Box::new(expr),
                },
            });
        }
        self.parse_postfix_expr()
    }

    fn parse_postfix_expr(&mut self) -> Result<CheckExpr, CftDiagnostics> {
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

    fn parse_type_predicate(&mut self) -> Result<TypePredicate, CftDiagnostics> {
        if let Some(span) = self.eat(&TokenKind::Null) {
            return Ok(TypePredicate::Null(span));
        }
        self.expect_ident().map(TypePredicate::Type)
    }

    fn peek_quantifier(&self) -> Option<QuantifierKind> {
        match self.peek().kind {
            TokenKind::All => Some(QuantifierKind::All),
            TokenKind::Any => Some(QuantifierKind::Any),
            TokenKind::None => Some(QuantifierKind::None),
            _ => None,
        }
    }

    fn eat_cmp_op(&mut self) -> Option<CmpOp> {
        if self.eat(&TokenKind::EqEq).is_some() {
            Some(CmpOp::Eq)
        } else if self.eat(&TokenKind::BangEq).is_some() {
            Some(CmpOp::Ne)
        } else if self.eat(&TokenKind::Less).is_some() {
            Some(CmpOp::Lt)
        } else if self.eat(&TokenKind::LessEq).is_some() {
            Some(CmpOp::Le)
        } else if self.eat(&TokenKind::Greater).is_some() {
            Some(CmpOp::Gt)
        } else if self.eat(&TokenKind::GreaterEq).is_some() {
            Some(CmpOp::Ge)
        } else {
            None
        }
    }
}

fn bin_expr(op: BinOp, lhs: CheckExpr, rhs: CheckExpr) -> CheckExpr {
    CheckExpr {
        span: lhs.span.join(rhs.span),
        kind: CheckExprKind::BinOp {
            op,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
        },
    }
}

fn validate_cmp_chain(
    module: &ModuleId,
    first_span: Span,
    rest: &[(CmpOp, CheckExpr)],
) -> Result<(), CftDiagnostics> {
    if rest.len() < 2 {
        return Ok(());
    }
    if rest.iter().any(|(op, _)| *op == CmpOp::Ne) {
        return Err(CftDiagnostics::one(CftDiagnostic::error(
            CftErrorCode::InvalidChainComparison,
            module.clone(),
            first_span,
            "`!=` cannot be used in chain comparisons",
        )));
    }
    let first_group = cmp_chain_group(rest[0].0);
    if matches!(first_group, CmpChainGroup::Equal | CmpChainGroup::NotEqual) {
        return Err(CftDiagnostics::one(CftDiagnostic::error(
            CftErrorCode::InvalidChainComparison,
            module.clone(),
            first_span,
            "chain comparison operators must be ordered comparisons",
        )));
    }
    if rest
        .iter()
        .skip(1)
        .any(|(op, _)| cmp_chain_group(*op) != first_group)
    {
        return Err(CftDiagnostics::one(CftDiagnostic::error(
            CftErrorCode::InvalidChainComparison,
            module.clone(),
            first_span,
            "chain comparison operators must have a consistent direction",
        )));
    }
    Ok(())
}

fn cmp_chain_group(op: CmpOp) -> CmpChainGroup {
    match op {
        CmpOp::Lt | CmpOp::Le => CmpChainGroup::Increasing,
        CmpOp::Gt | CmpOp::Ge => CmpChainGroup::Decreasing,
        CmpOp::Eq => CmpChainGroup::Equal,
        CmpOp::Ne => CmpChainGroup::NotEqual,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CmpChainGroup {
    Increasing,
    Decreasing,
    Equal,
    NotEqual,
}
