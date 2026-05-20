use crate::ast::{
    ArrayLiteral, AssignOp, AssignStmt, AssignTarget, BinaryExpr, BinaryOp, Block, CallExpr,
    ClassDecl, ClassField, ConfigDecl, ElseBranch, EnumDecl, Expr, FieldExpr, FnDecl, FnExpr,
    ForInStmt, Ident, IfStmt, ImportDecl, IndexExpr, Item, Literal, Module, OptionalFieldExpr,
    Param, Path, RecordEntry, RecordKey, RecordLiteral, ReturnStmt, Stmt, StringKind,
    StringLiteral, ThrowStmt, TryCatchStmt, TypeExpr, UnaryExpr, UnaryOp, VarDecl, WhileStmt,
    YieldStmt,
};
use crate::lexer::{lex, LexErrorKind, Token, TokenKind};
use crate::span::Span;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseOutput {
    pub module: Option<Module>,
    pub errors: Vec<ParseError>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub kind: ParseErrorKind,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseErrorKind {
    Lex(LexErrorKind),
    UnexpectedEof,
    UnexpectedToken,
    ExpectedItem,
    ExpectedType,
    ExpectedExpression,
    ExpectedIdentifier,
    ExpectedToken,
    InvalidAssignmentTarget,
    MissingCatch,
    UnsupportedParserNotImplemented,
}

pub fn parse_module(source: &str) -> ParseOutput {
    let lexed = lex(source);
    if !lexed.errors.is_empty() {
        return ParseOutput {
            module: None,
            errors: lexed
                .errors
                .into_iter()
                .map(|error| ParseError {
                    kind: ParseErrorKind::Lex(error.kind),
                    span: error.span,
                })
                .collect(),
        };
    }

    Parser::new(source, lexed.tokens).parse_module()
}

struct Parser<'a> {
    source: &'a str,
    tokens: Vec<Token>,
    pos: usize,
    errors: Vec<ParseError>,
}

impl<'a> Parser<'a> {
    fn new(source: &'a str, tokens: Vec<Token>) -> Self {
        Self {
            source,
            tokens,
            pos: 0,
            errors: Vec::new(),
        }
    }

    fn parse_module(mut self) -> ParseOutput {
        let mut items = Vec::new();

        while !self.is_eof() {
            let before = self.pos;
            match self.parse_item() {
                Some(item) => items.push(item),
                None => self.synchronize_top_level(),
            }
            if self.pos == before {
                self.bump();
            }
        }

        ParseOutput {
            module: Some(Module {
                items,
                span: Span {
                    start: 0,
                    end: self.source.len(),
                },
            }),
            errors: self.errors,
        }
    }

    fn parse_item(&mut self) -> Option<Item> {
        let local = self.eat(TokenKind::Local).is_some();
        match self.peek_kind() {
            Some(TokenKind::Import) if !local => self.parse_import().map(Item::Import),
            Some(TokenKind::Class) => self.parse_class(local).map(Item::Class),
            Some(TokenKind::Enum) => self.parse_enum(local).map(Item::Enum),
            Some(TokenKind::Var) => self.parse_var_decl(local).map(Item::Var),
            Some(TokenKind::Co) => {
                let start = self.current_span();
                self.bump();
                if !self.at(TokenKind::Fn) {
                    self.error(ParseErrorKind::ExpectedToken, start);
                    return None;
                }
                self.parse_fn_decl(local, true).map(Item::Function)
            }
            Some(TokenKind::Fn) => self.parse_fn_decl(local, false).map(Item::Function),
            Some(TokenKind::Ident) if !local => self.parse_config_decl().map(Item::Config),
            Some(_) => {
                self.error_here(ParseErrorKind::ExpectedItem);
                None
            }
            None => None,
        }
    }

    fn parse_import(&mut self) -> Option<ImportDecl> {
        let start = self.expect_token(TokenKind::Import)?.start;
        let module = self.parse_path()?;
        let alias = if self.eat(TokenKind::As).is_some() {
            Some(self.expect_ident()?)
        } else {
            None
        };
        let end = alias
            .as_ref()
            .map_or(module.span.end, |alias| alias.span.end);
        Some(ImportDecl {
            module,
            alias,
            span: Span { start, end },
        })
    }

    fn parse_config_decl(&mut self) -> Option<ConfigDecl> {
        let name = self.expect_ident()?;
        let ty = if self.eat(TokenKind::Colon).is_some() {
            Some(self.parse_type()?)
        } else {
            None
        };
        self.expect_token(TokenKind::Eq)?;
        let value = self.parse_expression()?;
        let span = Span {
            start: name.span.start,
            end: expr_span(&value).end,
        };
        Some(ConfigDecl {
            name,
            ty,
            value,
            span,
        })
    }

    fn parse_var_decl(&mut self, local: bool) -> Option<VarDecl> {
        let start = self.expect_token(TokenKind::Var)?.start;
        let name = self.expect_ident()?;
        let ty = if self.eat(TokenKind::Colon).is_some() {
            Some(self.parse_type()?)
        } else {
            None
        };
        let init = if self.eat(TokenKind::Eq).is_some() {
            Some(self.parse_expression()?)
        } else {
            None
        };
        let end = init.as_ref().map_or_else(
            || ty.as_ref().map_or(name.span.end, |ty| type_span(ty).end),
            |expr| expr_span(expr).end,
        );
        Some(VarDecl {
            local,
            name,
            ty,
            init,
            span: Span { start, end },
        })
    }

    fn parse_fn_decl(&mut self, local: bool, co: bool) -> Option<FnDecl> {
        let start = self.expect_token(TokenKind::Fn)?.start;
        let name = self.expect_ident()?;
        let params = self.parse_params()?;
        let body = self.parse_block()?;
        let span = Span {
            start,
            end: body.span.end,
        };
        Some(FnDecl {
            local,
            co,
            name,
            params,
            body,
            span,
        })
    }

    fn parse_class(&mut self, local: bool) -> Option<ClassDecl> {
        let start = self.expect_token(TokenKind::Class)?.start;
        let name = self.expect_ident()?;
        self.expect_token(TokenKind::LBrace)?;
        let mut fields = Vec::new();
        let mut validate = None;

        while !self.is_eof() && !self.at(TokenKind::RBrace) {
            if self.at(TokenKind::Validate) {
                self.bump();
                validate = self.parse_block();
                continue;
            }

            let before = self.pos;
            match self.parse_class_field() {
                Some(field) => fields.push(field),
                None => self.synchronize_class_member(),
            }
            if self.pos == before {
                self.bump();
            }
        }

        let end = self
            .expect_token(TokenKind::RBrace)
            .map_or(name.span.end, |span| span.end);
        Some(ClassDecl {
            local,
            name,
            fields,
            validate,
            span: Span { start, end },
        })
    }

    fn parse_class_field(&mut self) -> Option<ClassField> {
        let name = self.expect_ident()?;
        if self.eat(TokenKind::Colon).is_none() {
            self.error(ParseErrorKind::ExpectedType, self.current_span());
            return None;
        }
        let ty = self.parse_type()?;
        let default = if self.eat(TokenKind::Eq).is_some() {
            Some(self.parse_expression()?)
        } else {
            None
        };
        let end = default
            .as_ref()
            .map_or_else(|| type_span(&ty).end, |expr| expr_span(expr).end);
        let start = name.span.start;
        Some(ClassField {
            name,
            ty,
            default,
            span: Span { start, end },
        })
    }

    fn parse_enum(&mut self, local: bool) -> Option<EnumDecl> {
        let start = self.expect_token(TokenKind::Enum)?.start;
        let name = self.expect_ident()?;
        self.expect_token(TokenKind::LBrace)?;
        let mut variants = Vec::new();
        while !self.is_eof() && !self.at(TokenKind::RBrace) {
            if self.eat(TokenKind::Comma).is_some() {
                continue;
            }
            match self.expect_ident() {
                Some(variant) => variants.push(variant),
                None => {
                    self.synchronize_class_member();
                    break;
                }
            }
            self.eat(TokenKind::Comma);
        }
        let end = self
            .expect_token(TokenKind::RBrace)
            .map_or(name.span.end, |span| span.end);
        Some(EnumDecl {
            local,
            name,
            variants,
            span: Span { start, end },
        })
    }

    fn parse_block(&mut self) -> Option<Block> {
        let start = self.expect_token(TokenKind::LBrace)?.start;
        let mut stmts = Vec::new();

        while !self.is_eof() && !self.at(TokenKind::RBrace) {
            let before = self.pos;
            match self.parse_stmt() {
                Some(stmt) => stmts.push(stmt),
                None => self.synchronize_stmt(),
            }
            if self.pos == before {
                self.bump();
            }
        }

        let end = if let Some(span) = self.eat(TokenKind::RBrace) {
            span.end
        } else {
            self.error(ParseErrorKind::UnexpectedEof, self.eof_span());
            self.source.len()
        };
        Some(Block {
            stmts,
            span: Span { start, end },
        })
    }

    fn parse_stmt(&mut self) -> Option<Stmt> {
        match self.peek_kind()? {
            TokenKind::Local => {
                self.bump();
                if self.at(TokenKind::Var) {
                    self.parse_var_decl(true).map(Stmt::Var)
                } else if self.at(TokenKind::Fn) {
                    self.parse_fn_decl(true, false).map(Stmt::Function)
                } else if self.at(TokenKind::Co) {
                    let start = self.current_span();
                    self.bump();
                    if !self.at(TokenKind::Fn) {
                        self.error(ParseErrorKind::ExpectedToken, start);
                        None
                    } else {
                        self.parse_fn_decl(true, true).map(Stmt::Function)
                    }
                } else {
                    self.error_here(ParseErrorKind::UnexpectedToken);
                    None
                }
            }
            TokenKind::Var => self.parse_var_decl(false).map(Stmt::Var),
            TokenKind::Fn => self.parse_fn_decl(false, false).map(Stmt::Function),
            TokenKind::Co => {
                let start = self.current_span();
                self.bump();
                if !self.at(TokenKind::Fn) {
                    self.error(ParseErrorKind::ExpectedToken, start);
                    None
                } else {
                    self.parse_fn_decl(false, true).map(Stmt::Function)
                }
            }
            TokenKind::If => self.parse_if().map(Stmt::If),
            TokenKind::While => self.parse_while().map(Stmt::While),
            TokenKind::For => self.parse_for_in().map(Stmt::ForIn),
            TokenKind::Break => self.parse_break(),
            TokenKind::Continue => self.parse_continue(),
            TokenKind::Return => self.parse_return().map(Stmt::Return),
            TokenKind::Throw => self.parse_throw().map(Stmt::Throw),
            TokenKind::Try => self.parse_try_catch().map(Stmt::TryCatch),
            TokenKind::Yield => self.parse_yield().map(Stmt::Yield),
            _ => self.parse_expr_or_assignment_stmt(),
        }
    }

    fn parse_if(&mut self) -> Option<IfStmt> {
        let start = self.expect_token(TokenKind::If)?.start;
        let condition = self.parse_expression()?;
        let then_block = self.parse_block()?;
        let else_branch = if self.eat(TokenKind::Else).is_some() {
            if self.at(TokenKind::If) {
                Some(ElseBranch::If(Box::new(self.parse_if()?)))
            } else {
                Some(ElseBranch::Block(self.parse_block()?))
            }
        } else {
            None
        };
        let end = else_branch
            .as_ref()
            .map_or(then_block.span.end, |branch| match branch {
                ElseBranch::If(if_stmt) => if_stmt.span.end,
                ElseBranch::Block(block) => block.span.end,
            });
        Some(IfStmt {
            condition,
            then_block,
            else_branch,
            span: Span { start, end },
        })
    }

    fn parse_while(&mut self) -> Option<WhileStmt> {
        let start = self.expect_token(TokenKind::While)?.start;
        let condition = self.parse_expression()?;
        let body = self.parse_block()?;
        Some(WhileStmt {
            condition,
            span: Span {
                start,
                end: body.span.end,
            },
            body,
        })
    }

    fn parse_for_in(&mut self) -> Option<ForInStmt> {
        let start = self.expect_token(TokenKind::For)?.start;
        let item = self.expect_ident()?;
        self.expect_token(TokenKind::In)?;
        let iterable = self.parse_expression()?;
        let body = self.parse_block()?;
        Some(ForInStmt {
            item,
            iterable,
            span: Span {
                start,
                end: body.span.end,
            },
            body,
        })
    }

    fn parse_break(&mut self) -> Option<Stmt> {
        let span = self.expect_token(TokenKind::Break)?;
        if self.peek_kind().is_some_and(can_start_expression) {
            self.error_here(ParseErrorKind::UnexpectedToken);
            self.synchronize_stmt();
        }
        Some(Stmt::Break(span))
    }

    fn parse_continue(&mut self) -> Option<Stmt> {
        let span = self.expect_token(TokenKind::Continue)?;
        if self.peek_kind().is_some_and(can_start_expression) {
            self.error_here(ParseErrorKind::UnexpectedToken);
            self.synchronize_stmt();
        }
        Some(Stmt::Continue(span))
    }

    fn parse_return(&mut self) -> Option<ReturnStmt> {
        let start = self.expect_token(TokenKind::Return)?.start;
        let value = self.parse_expression()?;
        Some(ReturnStmt {
            span: Span {
                start,
                end: expr_span(&value).end,
            },
            value,
        })
    }

    fn parse_throw(&mut self) -> Option<ThrowStmt> {
        let start = self.expect_token(TokenKind::Throw)?.start;
        let value = self.parse_expression()?;
        Some(ThrowStmt {
            span: Span {
                start,
                end: expr_span(&value).end,
            },
            value,
        })
    }

    fn parse_try_catch(&mut self) -> Option<TryCatchStmt> {
        let start = self.expect_token(TokenKind::Try)?.start;
        let try_block = self.parse_block()?;
        if !self.at(TokenKind::Catch) {
            self.error(ParseErrorKind::MissingCatch, try_block.span);
            return None;
        }
        self.bump();
        let error_name = self.expect_ident()?;
        let catch_block = self.parse_block()?;
        Some(TryCatchStmt {
            try_block,
            error_name,
            span: Span {
                start,
                end: catch_block.span.end,
            },
            catch_block,
        })
    }

    fn parse_yield(&mut self) -> Option<YieldStmt> {
        let start = self.expect_token(TokenKind::Yield)?.start;
        if let Some(break_span) = self.eat(TokenKind::Break) {
            return Some(YieldStmt::Break {
                span: Span {
                    start,
                    end: break_span.end,
                },
            });
        }
        if self.eat(TokenKind::From).is_some() {
            let value = self.parse_expression()?;
            return Some(YieldStmt::From {
                span: Span {
                    start,
                    end: expr_span(&value).end,
                },
                value,
            });
        }
        let value = self.parse_expression()?;
        Some(YieldStmt::Value {
            span: Span {
                start,
                end: expr_span(&value).end,
            },
            value,
        })
    }

    fn parse_expr_or_assignment_stmt(&mut self) -> Option<Stmt> {
        let expr = self.parse_expression()?;
        if let Some((op, op_span)) = self.assignment_op() {
            self.bump();
            let target = match assign_target_from_expr(expr) {
                Some(target) => target,
                None => {
                    self.error(ParseErrorKind::InvalidAssignmentTarget, op_span);
                    let _ = self.parse_expression();
                    return None;
                }
            };
            let value = self.parse_expression()?;
            let span = Span {
                start: assign_target_span(&target).start,
                end: expr_span(&value).end,
            };
            Some(Stmt::Assign(AssignStmt {
                target,
                op,
                value,
                span,
            }))
        } else {
            Some(Stmt::Expr(expr))
        }
    }

    fn parse_type(&mut self) -> Option<TypeExpr> {
        match self.peek_kind() {
            Some(TokenKind::Ident) | Some(TokenKind::SelfKw) => {
                self.parse_path().map(TypeExpr::Name)
            }
            Some(TokenKind::LBracket) => {
                let start = self.bump()?.span.start;
                let key_or_element = self.parse_type()?;
                if self.eat(TokenKind::Colon).is_some() {
                    let value = self.parse_type()?;
                    let end = self.expect_token(TokenKind::RBracket)?.end;
                    Some(TypeExpr::Dict {
                        key: Box::new(key_or_element),
                        value: Box::new(value),
                        span: Span { start, end },
                    })
                } else {
                    let end = self.expect_token(TokenKind::RBracket)?.end;
                    Some(TypeExpr::Array {
                        element: Box::new(key_or_element),
                        span: Span { start, end },
                    })
                }
            }
            Some(_) => {
                self.error_here(ParseErrorKind::ExpectedType);
                None
            }
            None => {
                self.error(ParseErrorKind::UnexpectedEof, self.eof_span());
                None
            }
        }
    }

    fn parse_expression(&mut self) -> Option<Expr> {
        self.parse_expr_bp(0)
    }

    fn parse_expr_bp(&mut self, min_bp: u8) -> Option<Expr> {
        let mut lhs = self.parse_prefix()?;

        loop {
            match self.peek_kind() {
                Some(TokenKind::LParen) => {
                    let args = self.parse_expr_list(TokenKind::LParen, TokenKind::RParen)?;
                    let span = Span {
                        start: expr_span(&lhs).start,
                        end: self.previous_end(),
                    };
                    lhs = Expr::Call(CallExpr {
                        callee: Box::new(lhs),
                        args,
                        span,
                    });
                }
                Some(TokenKind::Dot) => {
                    self.bump();
                    let field = self.expect_ident()?;
                    let span = Span {
                        start: expr_span(&lhs).start,
                        end: field.span.end,
                    };
                    lhs = Expr::Field(FieldExpr {
                        object: Box::new(lhs),
                        field,
                        span,
                    });
                }
                Some(TokenKind::QuestionDot) => {
                    self.bump();
                    let field = self.expect_ident()?;
                    let span = Span {
                        start: expr_span(&lhs).start,
                        end: field.span.end,
                    };
                    lhs = Expr::OptionalField(OptionalFieldExpr {
                        object: Box::new(lhs),
                        field,
                        span,
                    });
                }
                Some(TokenKind::LBracket) => {
                    let start = expr_span(&lhs).start;
                    self.bump();
                    let index = self.parse_expression()?;
                    let end = self.expect_token(TokenKind::RBracket)?.end;
                    lhs = Expr::Index(IndexExpr {
                        object: Box::new(lhs),
                        index: Box::new(index),
                        span: Span { start, end },
                    });
                }
                _ => break,
            }
        }

        while let Some(kind) = self.peek_kind() {
            let Some((op, left_bp, right_bp)) = infix_binding_power(kind) else {
                break;
            };
            if left_bp < min_bp {
                break;
            }
            self.bump();
            let rhs = self.parse_expr_bp(right_bp)?;
            let span = Span {
                start: expr_span(&lhs).start,
                end: expr_span(&rhs).end,
            };
            lhs = Expr::Binary(BinaryExpr {
                lhs: Box::new(lhs),
                op,
                rhs: Box::new(rhs),
                span,
            });
        }

        Some(lhs)
    }

    fn parse_prefix(&mut self) -> Option<Expr> {
        match self.peek_kind() {
            Some(TokenKind::IntLiteral) => {
                let token = self.bump()?;
                Some(Expr::Literal(Literal::Int {
                    raw: self.slice(token.span).to_string(),
                    span: token.span,
                }))
            }
            Some(TokenKind::FloatLiteral) => {
                let token = self.bump()?;
                Some(Expr::Literal(Literal::Float {
                    raw: self.slice(token.span).to_string(),
                    span: token.span,
                }))
            }
            Some(
                TokenKind::StringLiteral
                | TokenKind::RawStringLiteral
                | TokenKind::MultilineStringLiteral
                | TokenKind::RawMultilineStringLiteral,
            ) => self
                .parse_string_literal()
                .map(Literal::String)
                .map(Expr::Literal),
            Some(TokenKind::True | TokenKind::False) => {
                let token = self.bump()?;
                Some(Expr::Literal(Literal::Bool {
                    value: token.kind == TokenKind::True,
                    span: token.span,
                }))
            }
            Some(TokenKind::Null) => {
                let token = self.bump()?;
                Some(Expr::Literal(Literal::Null { span: token.span }))
            }
            Some(TokenKind::Ident | TokenKind::SelfKw) => self.expect_ident().map(Expr::Name),
            Some(TokenKind::Minus) => {
                let start = self.bump()?.span.start;
                let expr = self.parse_expr_bp(9)?;
                let span = Span {
                    start,
                    end: expr_span(&expr).end,
                };
                Some(Expr::Unary(UnaryExpr {
                    op: UnaryOp::Neg,
                    expr: Box::new(expr),
                    span,
                }))
            }
            Some(TokenKind::Not) => {
                let start = self.bump()?.span.start;
                let expr = self.parse_expr_bp(9)?;
                let span = Span {
                    start,
                    end: expr_span(&expr).end,
                };
                Some(Expr::Unary(UnaryExpr {
                    op: UnaryOp::Not,
                    expr: Box::new(expr),
                    span,
                }))
            }
            Some(TokenKind::LParen) => {
                self.bump();
                let expr = self.parse_expression()?;
                self.expect_token(TokenKind::RParen)?;
                Some(expr)
            }
            Some(TokenKind::LBracket) => self.parse_array_literal(),
            Some(TokenKind::LBrace) => self.parse_record_literal(),
            Some(TokenKind::Fn) => self.parse_fn_expr(false).map(Expr::Fn),
            Some(TokenKind::Co) => {
                let start = self.current_span();
                self.bump();
                if !self.at(TokenKind::Fn) {
                    self.error(ParseErrorKind::ExpectedToken, start);
                    None
                } else {
                    self.parse_fn_expr(true).map(Expr::Fn)
                }
            }
            Some(_) => {
                self.error_here(ParseErrorKind::ExpectedExpression);
                None
            }
            None => {
                self.error(ParseErrorKind::ExpectedExpression, self.eof_span());
                None
            }
        }
    }

    fn parse_array_literal(&mut self) -> Option<Expr> {
        let start = self.expect_token(TokenKind::LBracket)?.start;
        let mut elements = Vec::new();
        if self.eat(TokenKind::RBracket).is_some() {
            return Some(Expr::Array(ArrayLiteral {
                elements,
                span: Span {
                    start,
                    end: self.previous_end(),
                },
            }));
        }
        loop {
            elements.push(self.parse_expression()?);
            if self.eat(TokenKind::Comma).is_none() {
                break;
            }
            if self.at(TokenKind::RBracket) {
                break;
            }
        }
        let end = self.expect_token(TokenKind::RBracket)?.end;
        Some(Expr::Array(ArrayLiteral {
            elements,
            span: Span { start, end },
        }))
    }

    fn parse_record_literal(&mut self) -> Option<Expr> {
        let start = self.expect_token(TokenKind::LBrace)?.start;
        let mut entries = Vec::new();
        if self.eat(TokenKind::RBrace).is_some() {
            return Some(Expr::Record(RecordLiteral {
                entries,
                span: Span {
                    start,
                    end: self.previous_end(),
                },
            }));
        }
        loop {
            let entry_start = self.current_span().start;
            let key = match self.peek_kind() {
                Some(TokenKind::Ident | TokenKind::SelfKw) => {
                    RecordKey::Ident(self.expect_ident()?)
                }
                Some(
                    TokenKind::StringLiteral
                    | TokenKind::RawStringLiteral
                    | TokenKind::MultilineStringLiteral
                    | TokenKind::RawMultilineStringLiteral,
                ) => RecordKey::String(self.parse_string_literal()?),
                _ => {
                    self.error_here(ParseErrorKind::ExpectedIdentifier);
                    return None;
                }
            };
            self.expect_token(TokenKind::Colon)?;
            let value = self.parse_expression()?;
            entries.push(RecordEntry {
                span: Span {
                    start: entry_start,
                    end: expr_span(&value).end,
                },
                key,
                value,
            });
            if self.eat(TokenKind::Comma).is_none() {
                break;
            }
            if self.at(TokenKind::RBrace) {
                break;
            }
        }
        let end = self.expect_token(TokenKind::RBrace)?.end;
        Some(Expr::Record(RecordLiteral {
            entries,
            span: Span { start, end },
        }))
    }

    fn parse_fn_expr(&mut self, co: bool) -> Option<FnExpr> {
        let start = self.expect_token(TokenKind::Fn)?.start;
        let params = self.parse_params()?;
        let body = self.parse_block()?;
        Some(FnExpr {
            co,
            params,
            span: Span {
                start,
                end: body.span.end,
            },
            body,
        })
    }

    fn parse_params(&mut self) -> Option<Vec<Param>> {
        self.expect_token(TokenKind::LParen)?;
        let mut params = Vec::new();
        if self.eat(TokenKind::RParen).is_some() {
            return Some(params);
        }
        loop {
            let name = self.expect_ident()?;
            let ty = if self.eat(TokenKind::Colon).is_some() {
                Some(self.parse_type()?)
            } else {
                None
            };
            let end = ty.as_ref().map_or(name.span.end, |ty| type_span(ty).end);
            params.push(Param {
                span: Span {
                    start: name.span.start,
                    end,
                },
                name,
                ty,
            });
            if self.eat(TokenKind::Comma).is_none() {
                break;
            }
            if self.at(TokenKind::RParen) {
                break;
            }
        }
        self.expect_token(TokenKind::RParen)?;
        Some(params)
    }

    fn parse_expr_list(&mut self, open: TokenKind, close: TokenKind) -> Option<Vec<Expr>> {
        self.expect_token(open)?;
        let mut exprs = Vec::new();
        if self.eat(close).is_some() {
            return Some(exprs);
        }
        loop {
            exprs.push(self.parse_expression()?);
            if self.eat(TokenKind::Comma).is_none() {
                break;
            }
            if self.at(close) {
                break;
            }
        }
        self.expect_token(close)?;
        Some(exprs)
    }

    fn parse_path(&mut self) -> Option<Path> {
        let first = self.expect_ident()?;
        let start = first.span.start;
        let mut end = first.span.end;
        let mut segments = vec![first];
        while self.eat(TokenKind::Dot).is_some() {
            let segment = self.expect_ident()?;
            end = segment.span.end;
            segments.push(segment);
        }
        Some(Path {
            segments,
            span: Span { start, end },
        })
    }

    fn parse_string_literal(&mut self) -> Option<StringLiteral> {
        let token = self.bump()?;
        let kind = match token.kind {
            TokenKind::StringLiteral => StringKind::Normal,
            TokenKind::RawStringLiteral => StringKind::Raw,
            TokenKind::MultilineStringLiteral => StringKind::Multiline,
            TokenKind::RawMultilineStringLiteral => StringKind::RawMultiline,
            _ => return None,
        };
        Some(StringLiteral {
            raw: self.slice(token.span).to_string(),
            kind,
            span: token.span,
        })
    }

    fn expect_ident(&mut self) -> Option<Ident> {
        match self.peek_kind() {
            Some(TokenKind::Ident | TokenKind::SelfKw) => {
                let token = self.bump()?;
                Some(Ident {
                    text: self.slice(token.span).to_string(),
                    span: token.span,
                })
            }
            Some(_) => {
                self.error_here(ParseErrorKind::ExpectedIdentifier);
                None
            }
            None => {
                self.error(ParseErrorKind::ExpectedIdentifier, self.eof_span());
                None
            }
        }
    }

    fn expect_token(&mut self, kind: TokenKind) -> Option<Span> {
        if self.at(kind) {
            self.bump().map(|token| token.span)
        } else if self.is_eof() {
            self.error(ParseErrorKind::UnexpectedEof, self.eof_span());
            None
        } else {
            self.error_here(ParseErrorKind::ExpectedToken);
            None
        }
    }

    fn assignment_op(&self) -> Option<(AssignOp, Span)> {
        let token = self.peek()?;
        let op = match token.kind {
            TokenKind::Eq => AssignOp::Assign,
            TokenKind::PlusEq => AssignOp::Add,
            TokenKind::MinusEq => AssignOp::Sub,
            TokenKind::StarEq => AssignOp::Mul,
            TokenKind::SlashEq => AssignOp::Div,
            TokenKind::PercentEq => AssignOp::Rem,
            TokenKind::QuestionQuestionEq => AssignOp::NullCoalesce,
            _ => return None,
        };
        Some((op, token.span))
    }

    fn synchronize_top_level(&mut self) {
        while let Some(kind) = self.peek_kind() {
            if matches!(
                kind,
                TokenKind::Import
                    | TokenKind::Local
                    | TokenKind::Class
                    | TokenKind::Enum
                    | TokenKind::Var
                    | TokenKind::Fn
                    | TokenKind::Co
                    | TokenKind::Ident
            ) {
                break;
            }
            self.bump();
        }
    }

    fn synchronize_class_member(&mut self) {
        while let Some(kind) = self.peek_kind() {
            if matches!(
                kind,
                TokenKind::RBrace | TokenKind::Validate | TokenKind::Ident
            ) {
                break;
            }
            self.bump();
        }
    }

    fn synchronize_stmt(&mut self) {
        let mut depth = 0usize;
        while let Some(kind) = self.peek_kind() {
            match kind {
                TokenKind::LBrace | TokenKind::LParen | TokenKind::LBracket => {
                    depth += 1;
                    self.bump();
                }
                TokenKind::RParen | TokenKind::RBracket => {
                    if depth == 0 {
                        break;
                    }
                    depth -= 1;
                    self.bump();
                }
                TokenKind::RBrace => break,
                kind if depth == 0 && can_start_statement(kind) => break,
                _ => {
                    self.bump();
                }
            }
        }
    }

    fn error_here(&mut self, kind: ParseErrorKind) {
        self.error(kind, self.current_span());
    }

    fn error(&mut self, kind: ParseErrorKind, span: Span) {
        self.errors.push(ParseError { kind, span });
    }

    fn at(&self, kind: TokenKind) -> bool {
        self.peek_kind() == Some(kind)
    }

    fn eat(&mut self, kind: TokenKind) -> Option<Span> {
        if self.at(kind) {
            self.bump().map(|token| token.span)
        } else {
            None
        }
    }

    fn peek_kind(&self) -> Option<TokenKind> {
        self.peek().map(|token| token.kind)
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn bump(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.pos)?.clone();
        self.pos += 1;
        Some(token)
    }

    fn is_eof(&self) -> bool {
        self.pos >= self.tokens.len()
    }

    fn current_span(&self) -> Span {
        self.peek()
            .map_or_else(|| self.eof_span(), |token| token.span)
    }

    fn eof_span(&self) -> Span {
        Span {
            start: self.source.len(),
            end: self.source.len(),
        }
    }

    fn previous_end(&self) -> usize {
        self.pos
            .checked_sub(1)
            .and_then(|index| self.tokens.get(index))
            .map_or(0, |token| token.span.end)
    }

    fn slice(&self, span: Span) -> &str {
        &self.source[span.start..span.end]
    }
}

fn can_start_expression(kind: TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::Ident
            | TokenKind::SelfKw
            | TokenKind::IntLiteral
            | TokenKind::FloatLiteral
            | TokenKind::StringLiteral
            | TokenKind::RawStringLiteral
            | TokenKind::MultilineStringLiteral
            | TokenKind::RawMultilineStringLiteral
            | TokenKind::True
            | TokenKind::False
            | TokenKind::Null
            | TokenKind::Minus
            | TokenKind::Not
            | TokenKind::LParen
            | TokenKind::LBracket
            | TokenKind::LBrace
            | TokenKind::Fn
            | TokenKind::Co
    )
}

fn can_start_statement(kind: TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::Local
            | TokenKind::Var
            | TokenKind::Fn
            | TokenKind::Co
            | TokenKind::If
            | TokenKind::While
            | TokenKind::For
            | TokenKind::Break
            | TokenKind::Continue
            | TokenKind::Return
            | TokenKind::Throw
            | TokenKind::Try
            | TokenKind::Yield
    ) || can_start_expression(kind)
}

fn infix_binding_power(kind: TokenKind) -> Option<(BinaryOp, u8, u8)> {
    Some(match kind {
        TokenKind::Or => (BinaryOp::Or, 1, 2),
        TokenKind::And => (BinaryOp::And, 3, 4),
        TokenKind::QuestionQuestion => (BinaryOp::NullCoalesce, 5, 5),
        TokenKind::EqEq => (BinaryOp::Eq, 6, 7),
        TokenKind::BangEq => (BinaryOp::NotEq, 6, 7),
        TokenKind::Lt => (BinaryOp::Lt, 6, 7),
        TokenKind::LtEq => (BinaryOp::LtEq, 6, 7),
        TokenKind::Gt => (BinaryOp::Gt, 6, 7),
        TokenKind::GtEq => (BinaryOp::GtEq, 6, 7),
        TokenKind::In => (BinaryOp::In, 6, 7),
        TokenKind::Plus => (BinaryOp::Add, 8, 9),
        TokenKind::Minus => (BinaryOp::Sub, 8, 9),
        TokenKind::Star => (BinaryOp::Mul, 10, 11),
        TokenKind::Slash => (BinaryOp::Div, 10, 11),
        TokenKind::Percent => (BinaryOp::Rem, 10, 11),
        _ => return None,
    })
}

fn assign_target_from_expr(expr: Expr) -> Option<AssignTarget> {
    match expr {
        Expr::Name(name) => Some(AssignTarget::Name(name)),
        Expr::Field(field) => Some(AssignTarget::Field {
            object: field.object,
            field: field.field,
            span: field.span,
        }),
        Expr::Index(index) => Some(AssignTarget::Index {
            object: index.object,
            index: index.index,
            span: index.span,
        }),
        _ => None,
    }
}

fn assign_target_span(target: &AssignTarget) -> Span {
    match target {
        AssignTarget::Name(name) => name.span,
        AssignTarget::Field { span, .. } | AssignTarget::Index { span, .. } => *span,
    }
}

fn expr_span(expr: &Expr) -> Span {
    match expr {
        Expr::Literal(literal) => literal_span(literal),
        Expr::Name(name) => name.span,
        Expr::Array(array) => array.span,
        Expr::Record(record) => record.span,
        Expr::Fn(func) => func.span,
        Expr::Unary(unary) => unary.span,
        Expr::Binary(binary) => binary.span,
        Expr::Call(call) => call.span,
        Expr::Field(field) => field.span,
        Expr::OptionalField(field) => field.span,
        Expr::Index(index) => index.span,
    }
}

fn literal_span(literal: &Literal) -> Span {
    match literal {
        Literal::Int { span, .. }
        | Literal::Float { span, .. }
        | Literal::Bool { span, .. }
        | Literal::Null { span } => *span,
        Literal::String(string) => string.span,
    }
}

fn type_span(ty: &TypeExpr) -> Span {
    match ty {
        TypeExpr::Name(path) => path.span,
        TypeExpr::Array { span, .. } | TypeExpr::Dict { span, .. } => *span,
    }
}
