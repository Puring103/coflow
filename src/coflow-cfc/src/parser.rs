use crate::ast::{
    BinOp, CheckBlock, CheckExpr, CheckExprKind, CmpOp, CondStmt, DataDef, EnumDef, EnumVariant,
    Expr, ExprKind, FieldDef, Item, ModuleAst, ObjectField, PathSegment, TypeDef, TypeName,
    TypeRef, UnaryOp, UseDecl,
};
use crate::container::ImportId;
use crate::error::ParseErrors;
use crate::lexer::{lex, Token, TokenKind};
use crate::span::Span;

pub fn parse_module(source: &str) -> Result<ModuleAst, ParseErrors> {
    let tokens = lex(source)?;
    Parser::new(tokens).parse_module()
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    next_import_id: u32,
}

enum RawPathPart {
    Field(String),
    Index(usize),
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens,
            pos: 0,
            next_import_id: 0,
        }
    }

    fn parse_module(&mut self) -> Result<ModuleAst, ParseErrors> {
        let mut imports = Vec::new();
        let mut items = Vec::new();
        let mut phase = 0;

        while !self.at(&TokenKind::Eof) {
            if self.at(&TokenKind::Use) {
                if phase > 0 {
                    return self.err("use declarations must appear before type, enum, and data");
                }
                imports.push(self.parse_use()?);
            } else if self.at(&TokenKind::Type) {
                if phase > 1 {
                    return self.err("type definitions must appear before data definitions");
                }
                phase = phase.max(1);
                items.push(Item::Type(self.parse_type()?));
            } else if self.at(&TokenKind::Enum) {
                if phase > 1 {
                    return self.err("enum definitions must appear before data definitions");
                }
                phase = phase.max(1);
                items.push(Item::Enum(self.parse_enum()?));
            } else if self.at(&TokenKind::Check) {
                phase = 2;
                items.push(Item::Check(self.parse_check_block()?));
            } else {
                phase = 2;
                items.push(Item::Data(self.parse_data()?));
            }
        }

        Ok(ModuleAst { imports, items })
    }

    fn parse_use(&mut self) -> Result<UseDecl, ParseErrors> {
        let start = self.expect_simple(&TokenKind::Use)?.start;
        let (path, _) = self.expect_string()?;
        self.expect_simple(&TokenKind::As)?;
        let (alias, _) = self.expect_ident()?;
        let end = self.expect_simple(&TokenKind::Semicolon)?.end;
        let id = ImportId(self.next_import_id);
        self.next_import_id += 1;
        Ok(UseDecl {
            id,
            path,
            alias,
            span: Span::new(start, end),
        })
    }

    fn parse_type(&mut self) -> Result<TypeDef, ParseErrors> {
        let start = self.expect_simple(&TokenKind::Type)?.start;
        let (name, _) = self.expect_ident()?;
        self.expect_simple(&TokenKind::LBrace)?;
        let mut fields = Vec::new();
        let mut check = None;
        while !self.at(&TokenKind::RBrace) {
            if self.at(&TokenKind::Check) {
                check = Some(self.parse_check_block()?);
                break;
            }
            let field_start = self.peek().span.start;
            let (field_name, _) = self.expect_ident()?;
            self.expect_simple(&TokenKind::Colon)?;
            let ty = self.parse_type_ref()?;
            let default = if self.eat(&TokenKind::Equal).is_some() {
                Some(self.parse_expr()?)
            } else {
                None
            };
            let end = self.expect_simple(&TokenKind::Semicolon)?.end;
            fields.push(FieldDef {
                name: field_name,
                ty,
                default,
                span: Span::new(field_start, end),
            });
        }
        let end = self.expect_simple(&TokenKind::RBrace)?.end;
        Ok(TypeDef {
            name,
            fields,
            check,
            span: Span::new(start, end),
        })
    }

    fn parse_enum(&mut self) -> Result<EnumDef, ParseErrors> {
        let start = self.expect_simple(&TokenKind::Enum)?.start;
        let (name, _) = self.expect_ident()?;
        self.expect_simple(&TokenKind::LBrace)?;
        let mut variants = Vec::new();
        while !self.at(&TokenKind::RBrace) {
            let span_start = self.peek().span.start;
            let (variant, _) = self.expect_ident()?;
            let value = if self.eat(&TokenKind::Equal).is_some() {
                Some(self.parse_signed_int()?)
            } else {
                None
            };
            let span_end = self.prev_span().end;
            variants.push(EnumVariant {
                name: variant,
                value,
                span: Span::new(span_start, span_end),
            });
            if self.eat(&TokenKind::Comma).is_none() {
                break;
            }
        }
        let end = self.expect_simple(&TokenKind::RBrace)?.end;
        Ok(EnumDef {
            name,
            variants,
            span: Span::new(start, end),
        })
    }

    fn parse_data(&mut self) -> Result<DataDef, ParseErrors> {
        let start = self.peek().span.start;
        let (name, _) = self.expect_ident()?;
        let ty = if self.eat(&TokenKind::Colon).is_some() {
            Some(self.parse_type_ref()?)
        } else {
            None
        };
        self.expect_simple(&TokenKind::Equal)?;
        let value = self.parse_expr()?;
        let end = self.expect_simple(&TokenKind::Semicolon)?.end;
        Ok(DataDef {
            name,
            ty,
            value,
            span: Span::new(start, end),
        })
    }

    fn parse_type_ref(&mut self) -> Result<TypeRef, ParseErrors> {
        if self.eat(&TokenKind::LBracket).is_some() {
            let inner = self.parse_type_ref()?;
            self.expect_simple(&TokenKind::RBracket)?;
            return Ok(TypeRef::Array(Box::new(inner)));
        }
        if self.eat(&TokenKind::LBrace).is_some() {
            let key = self.parse_type_ref()?;
            self.expect_simple(&TokenKind::Colon)?;
            let value = self.parse_type_ref()?;
            self.expect_simple(&TokenKind::RBrace)?;
            return Ok(TypeRef::Dict(Box::new(key), Box::new(value)));
        }
        let (name, _) = self.expect_ident()?;
        let ty = match name.as_str() {
            "int" => TypeRef::Int,
            "float" => TypeRef::Float,
            "bool" => TypeRef::Bool,
            "string" => TypeRef::String,
            "any" => TypeRef::Any,
            _ => {
                if self.eat(&TokenKind::Dot).is_some() {
                    let (member, _) = self.expect_ident()?;
                    TypeRef::Named(TypeName::Imported {
                        alias: name,
                        name: member,
                    })
                } else {
                    TypeRef::Named(TypeName::Local(name))
                }
            }
        };
        Ok(ty)
    }

    fn parse_expr(&mut self) -> Result<Expr, ParseErrors> {
        let token = self.peek().clone();
        match token.kind {
            TokenKind::Int(value) => {
                self.bump();
                Ok(Expr {
                    kind: ExprKind::Int(value),
                    span: token.span,
                })
            }
            TokenKind::Float(value) => {
                self.bump();
                Ok(Expr {
                    kind: ExprKind::Float(value),
                    span: token.span,
                })
            }
            TokenKind::True => {
                self.bump();
                Ok(Expr {
                    kind: ExprKind::Bool(true),
                    span: token.span,
                })
            }
            TokenKind::False => {
                self.bump();
                Ok(Expr {
                    kind: ExprKind::Bool(false),
                    span: token.span,
                })
            }
            TokenKind::String(value) => {
                self.bump();
                Ok(Expr {
                    kind: ExprKind::String(value),
                    span: token.span,
                })
            }
            TokenKind::Ident(_) => self.parse_path_or_qualified(),
            TokenKind::LBrace => self.parse_object(),
            TokenKind::LBracket => self.parse_array(),
            TokenKind::Dict => self.parse_dict(),
            TokenKind::Minus => {
                self.bump();
                let start = token.span.start;
                let next = self.peek().clone();
                match next.kind {
                    TokenKind::Int(value) => {
                        self.bump();
                        Ok(Expr {
                            kind: ExprKind::Int(-value),
                            span: Span::new(start, next.span.end),
                        })
                    }
                    TokenKind::Float(value) => {
                        self.bump();
                        Ok(Expr {
                            kind: ExprKind::Float(-value),
                            span: Span::new(start, next.span.end),
                        })
                    }
                    _ => self.err("expected number after `-`"),
                }
            }
            _ => self.err("expected expression"),
        }
    }

    fn parse_path_or_qualified(&mut self) -> Result<Expr, ParseErrors> {
        let start = self.peek().span.start;
        let (first, first_span) = self.expect_ident()?;
        let mut parts = Vec::new();
        let mut has_index = false;
        loop {
            if self.eat(&TokenKind::Dot).is_some() {
                let (part, _) = self.expect_ident()?;
                parts.push(RawPathPart::Field(part));
            } else if self.eat(&TokenKind::LBracket).is_some() {
                has_index = true;
                let index = self.parse_nonnegative_usize()?;
                self.expect_simple(&TokenKind::RBracket)?;
                parts.push(RawPathPart::Index(index));
            } else {
                break;
            }
        }
        let end = self.prev_span().end;
        let kind = if has_index || parts.len() > 1 {
            ExprKind::Path {
                root: first,
                segments: path_segments(parts),
            }
        } else {
            match parts.pop() {
                Some(RawPathPart::Field(part)) => ExprKind::Qualified(vec![first, part]),
                Some(RawPathPart::Index(index)) => ExprKind::Path {
                    root: first,
                    segments: vec![PathSegment::Index(index)],
                },
                None => ExprKind::Name(first),
            }
        };
        Ok(Expr {
            kind,
            span: Span::new(start.min(first_span.start), end),
        })
    }

    fn parse_object(&mut self) -> Result<Expr, ParseErrors> {
        let start = self.expect_simple(&TokenKind::LBrace)?.start;
        let mut fields = Vec::new();
        while !self.at(&TokenKind::RBrace) {
            let field_start = self.peek().span.start;
            let (name, _) = self.expect_ident()?;
            self.expect_simple(&TokenKind::Colon)?;
            let value = self.parse_expr()?;
            let field_end = value.span.end;
            fields.push(ObjectField {
                name,
                value,
                span: Span::new(field_start, field_end),
            });
            if self.eat(&TokenKind::Comma).is_none() {
                break;
            }
        }
        let end = self.expect_simple(&TokenKind::RBrace)?.end;
        Ok(Expr {
            kind: ExprKind::Object(fields),
            span: Span::new(start, end),
        })
    }

    fn parse_array(&mut self) -> Result<Expr, ParseErrors> {
        let start = self.expect_simple(&TokenKind::LBracket)?.start;
        let mut items = Vec::new();
        while !self.at(&TokenKind::RBracket) {
            items.push(self.parse_expr()?);
            if self.eat(&TokenKind::Comma).is_none() {
                break;
            }
        }
        let end = self.expect_simple(&TokenKind::RBracket)?.end;
        Ok(Expr {
            kind: ExprKind::Array(items),
            span: Span::new(start, end),
        })
    }

    fn parse_dict(&mut self) -> Result<Expr, ParseErrors> {
        let start = self.expect_simple(&TokenKind::Dict)?.start;
        self.expect_simple(&TokenKind::LBrace)?;
        let mut entries = Vec::new();
        while !self.at(&TokenKind::RBrace) {
            let key = self.parse_expr()?;
            self.expect_simple(&TokenKind::Colon)?;
            let value = self.parse_expr()?;
            entries.push((key, value));
            if self.eat(&TokenKind::Comma).is_none() {
                break;
            }
        }
        let end = self.expect_simple(&TokenKind::RBrace)?.end;
        Ok(Expr {
            kind: ExprKind::Dict(entries),
            span: Span::new(start, end),
        })
    }

    fn parse_check_block(&mut self) -> Result<CheckBlock, ParseErrors> {
        let start = self.expect_simple(&TokenKind::Check)?.start;
        self.expect_simple(&TokenKind::LBrace)?;
        let stmts = self.parse_cond_stmts()?;
        let end = self.expect_simple(&TokenKind::RBrace)?.end;
        Ok(CheckBlock {
            stmts,
            span: Span::new(start, end),
        })
    }

    fn parse_cond_stmts(&mut self) -> Result<Vec<CondStmt>, ParseErrors> {
        let mut stmts = Vec::new();
        while !self.at(&TokenKind::RBrace) {
            if self.at(&TokenKind::Eof) {
                return self.err("unterminated check block");
            }
            stmts.push(self.parse_cond_stmt()?);
        }
        Ok(stmts)
    }

    fn parse_cond_stmt(&mut self) -> Result<CondStmt, ParseErrors> {
        if self.at(&TokenKind::All) {
            return self.parse_all_stmt();
        }
        let expr = self.parse_check_expr()?;
        self.expect_simple(&TokenKind::Semicolon)?;
        Ok(CondStmt::Expr(expr))
    }

    fn parse_all_stmt(&mut self) -> Result<CondStmt, ParseErrors> {
        let start = self.expect_simple(&TokenKind::All)?.start;
        let (binding, _) = self.expect_ident()?;
        self.expect_simple(&TokenKind::In)?;
        let collection = self.parse_check_expr()?;
        self.expect_simple(&TokenKind::LBrace)?;
        let body = self.parse_cond_stmts()?;
        let end = self.expect_simple(&TokenKind::RBrace)?.end;
        Ok(CondStmt::All {
            binding,
            collection,
            body,
            span: Span::new(start, end),
        })
    }

    fn parse_check_expr(&mut self) -> Result<CheckExpr, ParseErrors> {
        self.parse_or_expr()
    }

    fn parse_or_expr(&mut self) -> Result<CheckExpr, ParseErrors> {
        let mut expr = self.parse_and_expr()?;
        while self.eat(&TokenKind::PipePipe).is_some() {
            let rhs = self.parse_and_expr()?;
            expr = bin_expr(BinOp::Or, expr, rhs);
        }
        Ok(expr)
    }

    fn parse_and_expr(&mut self) -> Result<CheckExpr, ParseErrors> {
        let mut expr = self.parse_cmp_chain()?;
        while self.eat(&TokenKind::AmpAmp).is_some() {
            let rhs = self.parse_cmp_chain()?;
            expr = bin_expr(BinOp::And, expr, rhs);
        }
        Ok(expr)
    }

    fn parse_bitor_expr(&mut self) -> Result<CheckExpr, ParseErrors> {
        let mut expr = self.parse_bitxor_expr()?;
        while self.eat(&TokenKind::Pipe).is_some() {
            let rhs = self.parse_bitxor_expr()?;
            expr = bin_expr(BinOp::BitOr, expr, rhs);
        }
        Ok(expr)
    }

    fn parse_bitxor_expr(&mut self) -> Result<CheckExpr, ParseErrors> {
        let mut expr = self.parse_bitand_expr()?;
        while self.eat(&TokenKind::Caret).is_some() {
            let rhs = self.parse_bitand_expr()?;
            expr = bin_expr(BinOp::BitXor, expr, rhs);
        }
        Ok(expr)
    }

    fn parse_bitand_expr(&mut self) -> Result<CheckExpr, ParseErrors> {
        let mut expr = self.parse_add_expr()?;
        while self.eat(&TokenKind::Amp).is_some() {
            let rhs = self.parse_add_expr()?;
            expr = bin_expr(BinOp::BitAnd, expr, rhs);
        }
        Ok(expr)
    }

    fn parse_cmp_chain(&mut self) -> Result<CheckExpr, ParseErrors> {
        let first = self.parse_bitor_expr()?;
        let mut rest = Vec::new();
        while let Some(op) = self.eat_cmp_op() {
            rest.push((op, self.parse_bitor_expr()?));
        }
        if rest.is_empty() {
            return Ok(first);
        }
        let start = first.span.start;
        validate_cmp_chain(&rest, first.span)?;
        let end = rest
            .last()
            .map_or(first.span.end, |(_, expr)| expr.span.end);
        Ok(CheckExpr {
            kind: CheckExprKind::CmpChain {
                first: Box::new(first),
                rest,
            },
            span: Span::new(start, end),
        })
    }

    fn parse_add_expr(&mut self) -> Result<CheckExpr, ParseErrors> {
        let mut expr = self.parse_shift_expr()?;
        loop {
            let op = if self.eat(&TokenKind::Plus).is_some() {
                BinOp::Add
            } else if self.eat(&TokenKind::Minus).is_some() {
                BinOp::Sub
            } else {
                break;
            };
            let rhs = self.parse_shift_expr()?;
            expr = bin_expr(op, expr, rhs);
        }
        Ok(expr)
    }

    fn parse_shift_expr(&mut self) -> Result<CheckExpr, ParseErrors> {
        let mut expr = self.parse_mul_expr()?;
        loop {
            let op = if self.eat(&TokenKind::LessLess).is_some() {
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

    fn parse_mul_expr(&mut self) -> Result<CheckExpr, ParseErrors> {
        let mut expr = self.parse_prefix_expr()?;
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
            let rhs = self.parse_prefix_expr()?;
            expr = bin_expr(op, expr, rhs);
        }
        Ok(expr)
    }

    fn parse_prefix_expr(&mut self) -> Result<CheckExpr, ParseErrors> {
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
            let expr = self.parse_prefix_expr()?;
            return Ok(CheckExpr {
                span: Span::new(token.span.start, expr.span.end),
                kind: CheckExprKind::Unary {
                    op,
                    expr: Box::new(expr),
                },
            });
        }
        self.parse_power_expr()
    }

    fn parse_power_expr(&mut self) -> Result<CheckExpr, ParseErrors> {
        let lhs = self.parse_postfix_expr()?;
        if self.eat(&TokenKind::StarStar).is_some() {
            let rhs = self.parse_prefix_expr()?;
            Ok(bin_expr(BinOp::Pow, lhs, rhs))
        } else {
            Ok(lhs)
        }
    }

    fn parse_postfix_expr(&mut self) -> Result<CheckExpr, ParseErrors> {
        let mut expr = self.parse_check_primary()?;
        loop {
            if self.eat(&TokenKind::Dot).is_some() {
                let (name, name_span) = self.expect_ident()?;
                let span = Span::new(expr.span.start, name_span.end);
                expr = CheckExpr {
                    kind: CheckExprKind::Field {
                        expr: Box::new(expr),
                        name,
                    },
                    span,
                };
            } else if self.eat(&TokenKind::LBracket).is_some() {
                let index = self.parse_check_expr()?;
                let end = self.expect_simple(&TokenKind::RBracket)?.end;
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

    fn parse_check_primary(&mut self) -> Result<CheckExpr, ParseErrors> {
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
            TokenKind::String(value) => {
                self.bump();
                Ok(CheckExpr {
                    kind: CheckExprKind::Str(value),
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
                let start = self.expect_simple(&TokenKind::LParen)?.start;
                let mut expr = self.parse_check_expr()?;
                let end = self.expect_simple(&TokenKind::RParen)?.end;
                expr.span = Span::new(start, end);
                Ok(expr)
            }
            _ => self.err("expected check expression"),
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

    fn parse_signed_int(&mut self) -> Result<i64, ParseErrors> {
        let sign = if self.eat(&TokenKind::Minus).is_some() {
            -1
        } else {
            1
        };
        let token = self.peek().clone();
        match token.kind {
            TokenKind::Int(value) => {
                self.bump();
                Ok(sign * value)
            }
            _ => self.err("expected integer literal"),
        }
    }

    fn parse_nonnegative_usize(&mut self) -> Result<usize, ParseErrors> {
        let token = self.peek().clone();
        match token.kind {
            TokenKind::Int(value) => {
                self.bump();
                usize::try_from(value)
                    .map_err(|_| ParseErrors::one("expected nonnegative integer index", token.span))
            }
            _ => self.err("expected nonnegative integer index"),
        }
    }

    fn expect_ident(&mut self) -> Result<(String, Span), ParseErrors> {
        let token = self.peek().clone();
        match token.kind {
            TokenKind::Ident(value) => {
                self.bump();
                Ok((value, token.span))
            }
            _ => self.err("expected identifier"),
        }
    }

    fn expect_string(&mut self) -> Result<(String, Span), ParseErrors> {
        let token = self.peek().clone();
        match token.kind {
            TokenKind::String(value) => {
                self.bump();
                Ok((value, token.span))
            }
            _ => self.err("expected string literal"),
        }
    }

    fn expect_simple(&mut self, kind: &TokenKind) -> Result<Span, ParseErrors> {
        if self.at(kind) {
            Ok(self.bump().span)
        } else {
            self.err(format!("expected `{}`", token_name(kind)))
        }
    }

    fn eat(&mut self, kind: &TokenKind) -> Option<Span> {
        if self.at(kind) {
            Some(self.bump().span)
        } else {
            None
        }
    }

    fn at(&self, kind: &TokenKind) -> bool {
        std::mem::discriminant(&self.peek().kind) == std::mem::discriminant(kind)
    }

    fn bump(&mut self) -> Token {
        let token = self.tokens[self.pos].clone();
        self.pos += 1;
        token
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.pos]
    }

    fn prev_span(&self) -> Span {
        self.tokens[self.pos - 1].span
    }

    fn err<T>(&self, message: impl Into<String>) -> Result<T, ParseErrors> {
        Err(ParseErrors::one(message, self.peek().span))
    }
}

fn token_name(kind: &TokenKind) -> &'static str {
    match kind {
        TokenKind::LBrace => "{",
        TokenKind::RBrace => "}",
        TokenKind::LBracket => "[",
        TokenKind::RBracket => "]",
        TokenKind::LParen => "(",
        TokenKind::RParen => ")",
        TokenKind::Colon => ":",
        TokenKind::Semicolon => ";",
        TokenKind::Comma => ",",
        TokenKind::Dot => ".",
        TokenKind::Equal => "=",
        TokenKind::In => "in",
        _ => "token",
    }
}

fn path_segments(parts: Vec<RawPathPart>) -> Vec<PathSegment> {
    parts
        .into_iter()
        .map(|part| match part {
            RawPathPart::Field(part) => PathSegment::Field(part),
            RawPathPart::Index(index) => PathSegment::Index(index),
        })
        .collect()
}

fn bin_expr(op: BinOp, lhs: CheckExpr, rhs: CheckExpr) -> CheckExpr {
    let span = Span::new(lhs.span.start, rhs.span.end);
    CheckExpr {
        kind: CheckExprKind::BinOp {
            op,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
        },
        span,
    }
}

fn validate_cmp_chain(rest: &[(CmpOp, CheckExpr)], span: Span) -> Result<(), ParseErrors> {
    if rest.len() < 2 {
        return Ok(());
    }
    if rest.iter().any(|(op, _)| *op == CmpOp::Ne) {
        return Err(ParseErrors::one(
            "`!=` cannot be used in chain comparisons",
            span,
        ));
    }
    let first_group = cmp_chain_group(rest[0].0);
    if rest
        .iter()
        .skip(1)
        .any(|(op, _)| cmp_chain_group(*op) != first_group)
    {
        return Err(ParseErrors::one(
            "chain comparison operators must have a consistent direction",
            span,
        ));
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
