use super::{Parsed, Parser};
use crate::diagnostics::{CftDiagnostics, CftErrorCode};
use crate::syntax::ast::{CheckExpr, CheckExprKind, CheckFormatSegment, NameRef, TypePredicate};
use crate::syntax::lexer::TokenKind;
use crate::syntax::Span;
use coflow_structure::StructureKind;

impl Parser<'_> {
    pub(super) fn parse_postfix_expr(&mut self) -> Result<Parsed<CheckExpr>, CftDiagnostics> {
        let mut expr = self.parse_primary_expr()?;
        loop {
            if let Some(opener) = self.eat(&TokenKind::LParen) {
                let CheckExpr { kind, span } = expr.value;
                let CheckExprKind::Name(name) = kind else {
                    return self.err(
                        CftErrorCode::UnexpectedToken,
                        "only named functions can be called",
                    );
                };
                let call_name = NameRef { name, span };
                let (args, end) = self.nested(StructureKind::CheckAst, opener, |parser| {
                    let mut args = Vec::new();
                    while !parser.at(&TokenKind::RParen) {
                        if parser.at(&TokenKind::Eof) {
                            return parser
                                .err(CftErrorCode::UnexpectedEof, "unterminated function call");
                        }
                        args.push(parser.parse_or_expr()?);
                        if parser.eat(&TokenKind::Comma).is_none() {
                            break;
                        }
                    }
                    let end = parser
                        .expect_simple(&TokenKind::RParen, CftErrorCode::ExpectedToken)?
                        .end;
                    Ok((args, end))
                })?;
                let depths = args.iter().map(|arg| arg.depth).collect::<Vec<_>>();
                expr = self.node(StructureKind::CheckAst, opener, depths, || CheckExpr {
                    span: Span::new(call_name.span.start, end),
                    kind: if call_name.name == "records" && args.len() == 1 {
                        let argument = &args[0].value;
                        if let CheckExprKind::Name(name) = &argument.kind {
                            CheckExprKind::Records {
                                type_name: NameRef {
                                    name: name.clone(),
                                    span: argument.span,
                                },
                            }
                        } else {
                            CheckExprKind::Call {
                                name: call_name,
                                args: args.into_iter().map(|arg| arg.value).collect(),
                            }
                        }
                    } else {
                        CheckExprKind::Call {
                            name: call_name,
                            args: args.into_iter().map(|arg| arg.value).collect(),
                        }
                    },
                })?;
            } else if self.at(&TokenKind::Question) && self.next_at(&TokenKind::Dot) {
                let question = self.bump().span;
                self.bump();
                let name = self.expect_ident()?;
                let span = Span::new(expr.value.span.start, name.span.end);
                let depth = expr.depth;
                expr = self.node(StructureKind::CheckAst, question, [depth], || CheckExpr {
                    span,
                    kind: CheckExprKind::SafeField {
                        expr: Box::new(expr.value),
                        name,
                    },
                })?;
            } else if self.at(&TokenKind::Question) && self.next_at(&TokenKind::LBracket) {
                let question = self.bump().span;
                let opener = self.bump().span;
                let (index, end) = self.nested(StructureKind::CheckAst, opener, |parser| {
                    let index = parser.parse_or_expr()?;
                    let end = parser
                        .expect_simple(&TokenKind::RBracket, CftErrorCode::ExpectedToken)?
                        .end;
                    Ok((index, end))
                })?;
                let span = Span::new(expr.value.span.start, end);
                let depths = [expr.depth, index.depth];
                expr = self.node(StructureKind::CheckAst, question, depths, || CheckExpr {
                    span,
                    kind: CheckExprKind::SafeIndex {
                        expr: Box::new(expr.value),
                        index: Box::new(index.value),
                    },
                })?;
            } else if let Some(dot) = self.eat(&TokenKind::Dot) {
                let name = self.expect_ident()?;
                if let Some(opener) = self.eat(&TokenKind::LParen) {
                    let (args, end) = self.nested(StructureKind::CheckAst, opener, |parser| {
                        let mut args = Vec::new();
                        if !parser.at(&TokenKind::RParen) {
                            loop {
                                args.push(parser.parse_or_expr()?);
                                if parser.eat(&TokenKind::Comma).is_none() {
                                    break;
                                }
                            }
                        }
                        let end = parser
                            .expect_simple(&TokenKind::RParen, CftErrorCode::ExpectedToken)?
                            .end;
                        Ok((args, end))
                    })?;
                    let span = Span::new(expr.value.span.start, end);
                    let depths = std::iter::once(expr.depth)
                        .chain(args.iter().map(|arg| arg.depth))
                        .collect::<Vec<_>>();
                    expr = self.node(StructureKind::CheckAst, opener, depths, || CheckExpr {
                        span,
                        kind: CheckExprKind::MethodCall {
                            receiver: Box::new(expr.value),
                            name,
                            args: args.into_iter().map(|arg| arg.value).collect(),
                        },
                    })?;
                } else {
                    let span = Span::new(expr.value.span.start, name.span.end);
                    let depth = expr.depth;
                    expr = self.node(StructureKind::CheckAst, dot, [depth], || CheckExpr {
                        span,
                        kind: CheckExprKind::Field {
                            expr: Box::new(expr.value),
                            name,
                        },
                    })?;
                }
            } else if let Some(opener) = self.eat(&TokenKind::LBracket) {
                let (index, end) = self.nested(StructureKind::CheckAst, opener, |parser| {
                    let index = parser.parse_or_expr()?;
                    let end = parser
                        .expect_simple(&TokenKind::RBracket, CftErrorCode::ExpectedToken)?
                        .end;
                    Ok((index, end))
                })?;
                let span = Span::new(expr.value.span.start, end);
                let depths = [expr.depth, index.depth];
                expr = self.node(StructureKind::CheckAst, opener, depths, || CheckExpr {
                    span,
                    kind: CheckExprKind::Index {
                        expr: Box::new(expr.value),
                        index: Box::new(index.value),
                    },
                })?;
            } else {
                break;
            }
        }
        Ok(expr)
    }

    fn parse_primary_expr(&mut self) -> Result<Parsed<CheckExpr>, CftDiagnostics> {
        let token = self.peek().clone();
        match token.kind {
            TokenKind::Int(value) => {
                self.bump();
                self.node(StructureKind::CheckAst, token.span, [], || CheckExpr {
                    kind: CheckExprKind::Int(value),
                    span: token.span,
                })
            }
            TokenKind::Float(value) => {
                self.bump();
                self.node(StructureKind::CheckAst, token.span, [], || CheckExpr {
                    kind: CheckExprKind::Float(value),
                    span: token.span,
                })
            }
            TokenKind::True => {
                self.bump();
                self.node(StructureKind::CheckAst, token.span, [], || CheckExpr {
                    kind: CheckExprKind::Bool(true),
                    span: token.span,
                })
            }
            TokenKind::False => {
                self.bump();
                self.node(StructureKind::CheckAst, token.span, [], || CheckExpr {
                    kind: CheckExprKind::Bool(false),
                    span: token.span,
                })
            }
            TokenKind::Null => {
                self.bump();
                self.node(StructureKind::CheckAst, token.span, [], || CheckExpr {
                    kind: CheckExprKind::Null,
                    span: token.span,
                })
            }
            TokenKind::String(value) => {
                self.bump();
                self.node(StructureKind::CheckAst, token.span, [], || CheckExpr {
                    kind: CheckExprKind::String(value),
                    span: token.span,
                })
            }
            TokenKind::FormattedStringStart => self.parse_formatted_string(),
            TokenKind::Ident(value) => {
                self.bump();
                self.node(StructureKind::CheckAst, token.span, [], || CheckExpr {
                    kind: CheckExprKind::Name(value),
                    span: token.span,
                })
            }
            TokenKind::LParen => {
                let opener = self.expect_simple(&TokenKind::LParen, CftErrorCode::ExpectedToken)?;
                let (mut expr, end) = self.nested(StructureKind::CheckAst, opener, |parser| {
                    let expr = parser.parse_or_expr()?;
                    let end = parser
                        .expect_simple(&TokenKind::RParen, CftErrorCode::ExpectedToken)?
                        .end;
                    Ok((expr, end))
                })?;
                expr.value.span = Span::new(opener.start, end);
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

    pub(super) fn parse_formatted_string(&mut self) -> Result<Parsed<CheckExpr>, CftDiagnostics> {
        let start = self
            .expect_simple(
                &TokenKind::FormattedStringStart,
                CftErrorCode::ExpectedToken,
            )?
            .start;
        let mut segments = Vec::new();
        let mut depths = Vec::new();
        while !self.at(&TokenKind::FormattedStringEnd) {
            let token = self.peek().clone();
            match token.kind {
                TokenKind::FormattedStringText(value) => {
                    self.bump();
                    segments.push(CheckFormatSegment::Text(value, token.span));
                }
                TokenKind::FormattedStringExprStart => {
                    let opener = self.bump().span;
                    let expr = self.nested(StructureKind::CheckAst, opener, |parser| {
                        parser.parse_or_expr()
                    })?;
                    self.expect_simple(
                        &TokenKind::FormattedStringExprEnd,
                        CftErrorCode::ExpectedToken,
                    )?;
                    depths.push(expr.depth);
                    segments.push(CheckFormatSegment::Expr(expr.value));
                }
                TokenKind::Eof => {
                    return self.err(CftErrorCode::UnexpectedEof, "unterminated formatted string");
                }
                _ => {
                    return self.err(
                        CftErrorCode::InvalidCheckStatement,
                        "invalid formatted string segment",
                    );
                }
            }
        }
        let end = self
            .expect_simple(&TokenKind::FormattedStringEnd, CftErrorCode::ExpectedToken)?
            .end;
        let span = Span::new(start, end);
        self.node(StructureKind::CheckAst, span, depths, || CheckExpr {
            kind: CheckExprKind::FormattedString(segments),
            span,
        })
    }

    pub(super) fn parse_type_predicate(&mut self) -> Result<TypePredicate, CftDiagnostics> {
        if let Some(span) = self.eat(&TokenKind::Null) {
            return Ok(TypePredicate::Null(span));
        }
        self.expect_ident().map(TypePredicate::Type)
    }
}
