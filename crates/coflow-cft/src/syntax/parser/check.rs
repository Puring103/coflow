use super::{negate_u64_to_i64, Parsed, Parser};
use crate::diagnostics::{CftDiagnostic, CftDiagnostics, CftErrorCode};
use crate::module::ModuleId;
use crate::syntax::ast::{
    BinOp, CheckBlock, CheckExpr, CheckExprKind, CheckMessage, CheckStmt, CmpOp, QuantifierKind,
    TypePredicate, UnaryOp,
};
use crate::syntax::lexer::TokenKind;
use crate::syntax::Span;
use coflow_structure::StructureKind;

impl Parser<'_> {
    pub(super) fn parse_check_block(&mut self) -> Result<CheckBlock, CftDiagnostics> {
        let start = self
            .expect_simple(&TokenKind::Check, CftErrorCode::UnexpectedToken)?
            .start;
        self.expect_simple(&TokenKind::LBrace, CftErrorCode::ExpectedToken)?;
        let stmts = self.parse_check_stmts()?;
        let end = self
            .expect_simple(&TokenKind::RBrace, CftErrorCode::ExpectedToken)?
            .end;
        let span = Span::new(start, end);
        self.node(StructureKind::CheckAst, span, [stmts.depth], || {
            CheckBlock {
                stmts: stmts.value,
                span,
            }
        })
        .map(|block| block.value)
    }

    fn parse_check_stmts(&mut self) -> Result<Parsed<Vec<CheckStmt>>, CftDiagnostics> {
        let mut stmts = Vec::new();
        while !self.at(&TokenKind::RBrace) {
            if self.at(&TokenKind::Eof) {
                return self.err(CftErrorCode::UnexpectedEof, "unterminated check block");
            }
            stmts.push(self.parse_check_stmt()?);
        }
        let depth = stmts
            .iter()
            .map(|stmt: &Parsed<CheckStmt>| stmt.depth)
            .max()
            .unwrap_or(0);
        Ok(Parsed {
            value: stmts.into_iter().map(|stmt| stmt.value).collect(),
            depth,
        })
    }

    fn parse_check_stmt(&mut self) -> Result<Parsed<CheckStmt>, CftDiagnostics> {
        if let Some(kind) = self.peek_quantifier() {
            return self.parse_quantifier_stmt(kind);
        }
        if self.at(&TokenKind::When) {
            return self.parse_when_stmt();
        }
        let expr = self.parse_or_expr()?;
        let message = if self.eat(&TokenKind::Colon).is_some() {
            let token = self.peek().clone();
            let TokenKind::String(value) = token.kind else {
                return self.err_at(
                    CftErrorCode::InvalidCheckStatement,
                    token.span,
                    "check message must be a string literal",
                );
            };
            self.bump();
            Some(CheckMessage {
                value,
                span: token.span,
            })
        } else {
            None
        };
        if self.eat(&TokenKind::Semicolon).is_none() {
            return self.err(
                CftErrorCode::InvalidCheckStatement,
                "check expression statements must end with `;`",
            );
        }
        let span = expr
            .value
            .span
            .join(message.as_ref().map_or(expr.value.span, |message| message.span));
        self.node(StructureKind::CheckAst, span, [expr.depth], || {
            CheckStmt::Expr {
                condition: expr.value,
                message,
                span,
            }
        })
    }

    fn parse_quantifier_stmt(
        &mut self,
        kind: QuantifierKind,
    ) -> Result<Parsed<CheckStmt>, CftDiagnostics> {
        let keyword = self.bump().span;
        let start = keyword.start;
        let binding = self.expect_ident()?;
        self.expect_simple(&TokenKind::In, CftErrorCode::ExpectedToken)?;
        let collection = self.parse_or_expr()?;
        self.expect_simple(&TokenKind::LBrace, CftErrorCode::ExpectedToken)?;
        let body = self.nested(StructureKind::CheckAst, keyword, |parser| {
            parser.parse_check_stmts()
        })?;
        let end = self
            .expect_simple(&TokenKind::RBrace, CftErrorCode::ExpectedToken)?
            .end;
        let span = Span::new(start, end);
        self.node(
            StructureKind::CheckAst,
            keyword,
            [collection.depth, body.depth],
            || CheckStmt::Quantifier {
                kind,
                binding,
                collection: collection.value,
                body: body.value,
                span,
            },
        )
    }

    fn parse_when_stmt(&mut self) -> Result<Parsed<CheckStmt>, CftDiagnostics> {
        let keyword = self.expect_simple(&TokenKind::When, CftErrorCode::UnexpectedToken)?;
        let start = keyword.start;
        let condition = self.parse_or_expr()?;
        self.expect_simple(&TokenKind::LBrace, CftErrorCode::ExpectedToken)?;
        let body = self.nested(StructureKind::CheckAst, keyword, |parser| {
            parser.parse_check_stmts()
        })?;
        let end = self
            .expect_simple(&TokenKind::RBrace, CftErrorCode::ExpectedToken)?
            .end;
        let span = Span::new(start, end);
        self.node(
            StructureKind::CheckAst,
            keyword,
            [condition.depth, body.depth],
            || CheckStmt::When {
                condition: condition.value,
                body: body.value,
                span,
            },
        )
    }

    pub(super) fn parse_or_expr(&mut self) -> Result<Parsed<CheckExpr>, CftDiagnostics> {
        let mut expr = self.parse_and_expr()?;
        while let Some(operator) = self.eat(&TokenKind::PipePipe) {
            let rhs = self.parse_and_expr()?;
            expr = self.bin_expr(operator, BinOp::Or, expr, rhs)?;
        }
        Ok(expr)
    }

    fn parse_and_expr(&mut self) -> Result<Parsed<CheckExpr>, CftDiagnostics> {
        let mut expr = self.parse_is_expr()?;
        while let Some(operator) = self.eat(&TokenKind::AmpAmp) {
            let rhs = self.parse_is_expr()?;
            expr = self.bin_expr(operator, BinOp::And, expr, rhs)?;
        }
        Ok(expr)
    }

    fn parse_is_expr(&mut self) -> Result<Parsed<CheckExpr>, CftDiagnostics> {
        let mut expr = self.parse_cmp_chain()?;
        while let Some(operator) = self.eat(&TokenKind::Is) {
            let predicate = self.parse_type_predicate()?;
            let end = match &predicate {
                TypePredicate::Type(name) => name.span.end,
                TypePredicate::Null(span) => span.end,
            };
            let span = Span::new(expr.value.span.start, end);
            let depth = expr.depth;
            expr = self.node(StructureKind::CheckAst, operator, [depth], || CheckExpr {
                span,
                kind: CheckExprKind::Is {
                    expr: Box::new(expr.value),
                    predicate,
                },
            })?;
        }
        Ok(expr)
    }

    fn parse_cmp_chain(&mut self) -> Result<Parsed<CheckExpr>, CftDiagnostics> {
        let first = self.parse_bitor_expr()?;
        let mut rest = Vec::new();
        while let Some(op) = self.eat_cmp_op() {
            rest.push((op, self.parse_bitor_expr()?));
        }
        if rest.is_empty() {
            return Ok(first);
        }
        validate_cmp_chain(self.module, first.value.span, &rest)?;
        let end = rest
            .last()
            .map_or(first.value.span.end, |(_, expr)| expr.value.span.end);
        let span = Span::new(first.value.span.start, end);
        let depths = std::iter::once(first.depth)
            .chain(rest.iter().map(|(_, expr)| expr.depth))
            .collect::<Vec<_>>();
        self.node(StructureKind::CheckAst, span, depths, || CheckExpr {
            span,
            kind: CheckExprKind::CmpChain {
                first: Box::new(first.value),
                rest: rest
                    .into_iter()
                    .map(|(op, expr)| (op, expr.value))
                    .collect(),
            },
        })
    }

    fn parse_bitor_expr(&mut self) -> Result<Parsed<CheckExpr>, CftDiagnostics> {
        let mut expr = self.parse_add_expr()?;
        loop {
            let operator = if let Some(span) = self.eat(&TokenKind::Pipe) {
                (span, BinOp::BitOr)
            } else if let Some(span) = self.eat(&TokenKind::Caret) {
                (span, BinOp::BitXor)
            } else if let Some(span) = self.eat(&TokenKind::Amp) {
                (span, BinOp::BitAnd)
            } else {
                break;
            };
            let rhs = self.parse_add_expr()?;
            expr = self.bin_expr(operator.0, operator.1, expr, rhs)?;
        }
        Ok(expr)
    }

    fn parse_add_expr(&mut self) -> Result<Parsed<CheckExpr>, CftDiagnostics> {
        let mut expr = self.parse_mul_expr()?;
        loop {
            let operator = if let Some(span) = self.eat(&TokenKind::Plus) {
                (span, BinOp::Add)
            } else if let Some(span) = self.eat(&TokenKind::Minus) {
                (span, BinOp::Sub)
            } else if let Some(span) = self.eat(&TokenKind::LessLess) {
                (span, BinOp::Shl)
            } else if let Some(span) = self.eat(&TokenKind::GreaterGreater) {
                (span, BinOp::Shr)
            } else {
                break;
            };
            let rhs = self.parse_mul_expr()?;
            expr = self.bin_expr(operator.0, operator.1, expr, rhs)?;
        }
        Ok(expr)
    }

    fn parse_mul_expr(&mut self) -> Result<Parsed<CheckExpr>, CftDiagnostics> {
        let mut expr = self.parse_power_expr()?;
        loop {
            let operator = if let Some(span) = self.eat(&TokenKind::Star) {
                (span, BinOp::Mul)
            } else if let Some(span) = self.eat(&TokenKind::Slash) {
                (span, BinOp::Div)
            } else if let Some(span) = self.eat(&TokenKind::SlashSlash) {
                (span, BinOp::IntDiv)
            } else if let Some(span) = self.eat(&TokenKind::Percent) {
                (span, BinOp::Mod)
            } else {
                break;
            };
            let rhs = self.parse_power_expr()?;
            expr = self.bin_expr(operator.0, operator.1, expr, rhs)?;
        }
        Ok(expr)
    }

    fn parse_power_expr(&mut self) -> Result<Parsed<CheckExpr>, CftDiagnostics> {
        let lhs = self.parse_prefix_expr()?;
        if let Some(operator) = self.eat(&TokenKind::StarStar) {
            let rhs = self.nested(StructureKind::CheckAst, operator, |parser| {
                parser.parse_power_expr()
            })?;
            self.bin_expr(operator, BinOp::Pow, lhs, rhs)
        } else {
            Ok(lhs)
        }
    }

    fn parse_prefix_expr(&mut self) -> Result<Parsed<CheckExpr>, CftDiagnostics> {
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
                    return self.node(StructureKind::CheckAst, span, [], || CheckExpr {
                        kind: CheckExprKind::Int(negated),
                        span,
                    });
                }
            }
            let expr = self.nested(StructureKind::CheckAst, token.span, |parser| {
                parser.parse_prefix_expr()
            })?;
            let span = Span::new(token.span.start, expr.value.span.end);
            let depth = expr.depth;
            return self.node(StructureKind::CheckAst, token.span, [depth], || CheckExpr {
                span,
                kind: CheckExprKind::Unary {
                    op,
                    expr: Box::new(expr.value),
                },
            });
        }
        self.parse_postfix_expr()
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

    fn bin_expr(
        &mut self,
        trigger: Span,
        op: BinOp,
        lhs: Parsed<CheckExpr>,
        rhs: Parsed<CheckExpr>,
    ) -> Result<Parsed<CheckExpr>, CftDiagnostics> {
        let span = lhs.value.span.join(rhs.value.span);
        let depths = [lhs.depth, rhs.depth];
        self.node(StructureKind::CheckAst, trigger, depths, || CheckExpr {
            span,
            kind: CheckExprKind::BinOp {
                op,
                lhs: Box::new(lhs.value),
                rhs: Box::new(rhs.value),
            },
        })
    }
}

fn validate_cmp_chain(
    module: &ModuleId,
    first_span: Span,
    rest: &[(CmpOp, Parsed<CheckExpr>)],
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
