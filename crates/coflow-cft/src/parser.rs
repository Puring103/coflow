use crate::ast::{
    Annotation, AnnotationArg, BinOp, CheckBlock, CheckExpr, CheckExprKind, CheckStmt, CmpOp,
    ConstDef, ConstLiteral, DefaultExpr, DefaultExprKind, EnumDef, EnumVariant, FieldDef, Item,
    ModuleAst, NameRef, QuantifierKind, SignedInt, TypeDef, TypePredicate, TypeRef, TypeRefKind,
    UnaryOp,
};
use crate::container::ModuleId;
use crate::error::{CftDiagnostic, CftDiagnostics, CftErrorCode};
use crate::lexer::{lex, Token, TokenKind};
use crate::span::Span;

pub fn parse_module(module: &ModuleId, source: &str) -> Result<ModuleAst, CftDiagnostics> {
    let tokens = lex(module, source)?;
    Parser::new(module, tokens).parse_module()
}

struct Parser<'a> {
    module: &'a ModuleId,
    tokens: Vec<Token>,
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(module: &'a ModuleId, tokens: Vec<Token>) -> Self {
        Self {
            module,
            tokens,
            pos: 0,
        }
    }

    fn parse_module(&mut self) -> Result<ModuleAst, CftDiagnostics> {
        let mut items = Vec::new();
        let mut pending_annotations = Vec::new();
        while !self.at(&TokenKind::Eof) {
            while self.at(&TokenKind::At) {
                pending_annotations.push(self.parse_annotation()?);
            }
            if self.at(&TokenKind::Eof) {
                break;
            }
            if self.at(&TokenKind::Const) {
                items.push(Item::Const(
                    self.parse_const(std::mem::take(&mut pending_annotations))?,
                ));
            } else if self.at(&TokenKind::Enum) {
                items.push(Item::Enum(
                    self.parse_enum(std::mem::take(&mut pending_annotations))?,
                ));
            } else if self.at(&TokenKind::Type)
                || self.at(&TokenKind::Abstract)
                || self.at(&TokenKind::Sealed)
            {
                items.push(Item::Type(
                    self.parse_type(std::mem::take(&mut pending_annotations))?,
                ));
            } else {
                return self.err(
                    CftErrorCode::InvalidTopLevelItem,
                    "top level items must be const, enum, or type definitions",
                );
            }
        }
        Ok(ModuleAst {
            items,
            dangling_annotations: pending_annotations,
        })
    }

    fn parse_annotation(&mut self) -> Result<Annotation, CftDiagnostics> {
        let start = self
            .expect_simple(&TokenKind::At, CftErrorCode::InvalidAnnotationSyntax)?
            .start;
        let name = self.expect_ident_with_code(CftErrorCode::InvalidAnnotationSyntax)?;
        let mut args = Vec::new();
        let mut end = name.span.end;
        if self.eat(&TokenKind::LParen).is_some() {
            while !self.at(&TokenKind::RParen) {
                if self.at(&TokenKind::Eof) {
                    return self.err_at(
                        CftErrorCode::InvalidAnnotationSyntax,
                        Span::new(start, end),
                        "unterminated annotation argument list",
                    );
                }
                args.push(self.parse_annotation_arg()?);
                if self.eat(&TokenKind::Comma).is_none() {
                    break;
                }
            }
            end = self
                .expect_simple(&TokenKind::RParen, CftErrorCode::InvalidAnnotationSyntax)?
                .end;
        }
        Ok(Annotation {
            name: name.name,
            name_span: name.span,
            args,
            span: Span::new(start, end),
        })
    }

    fn parse_annotation_arg(&mut self) -> Result<AnnotationArg, CftDiagnostics> {
        let token = self.peek().clone();
        match token.kind {
            TokenKind::Ident(_) => self
                .expect_ident_with_code(CftErrorCode::InvalidAnnotationSyntax)
                .map(AnnotationArg::Name),
            TokenKind::String(value) => {
                self.bump();
                Ok(AnnotationArg::String(value, token.span))
            }
            TokenKind::Int(value) => {
                self.bump();
                Ok(AnnotationArg::Int(value, token.span))
            }
            TokenKind::Float(value) => {
                self.bump();
                Ok(AnnotationArg::Float(value, token.span))
            }
            TokenKind::True => {
                self.bump();
                Ok(AnnotationArg::Bool(true, token.span))
            }
            TokenKind::False => {
                self.bump();
                Ok(AnnotationArg::Bool(false, token.span))
            }
            TokenKind::Null => {
                self.bump();
                Ok(AnnotationArg::Null(token.span))
            }
            TokenKind::UIntOverflow(_) => self.err(
                CftErrorCode::InvalidIntLiteral,
                "integer literal out of range",
            ),
            _ => self.err(
                CftErrorCode::InvalidAnnotationSyntax,
                "invalid annotation argument",
            ),
        }
    }

    fn parse_const(&mut self, annotations: Vec<Annotation>) -> Result<ConstDef, CftDiagnostics> {
        let start = self
            .expect_simple(&TokenKind::Const, CftErrorCode::UnexpectedToken)?
            .start;
        let name = self.expect_ident()?;
        let ty = if self.eat(&TokenKind::Colon).is_some() {
            Some(self.parse_type_ref()?)
        } else {
            None
        };
        self.expect_simple(&TokenKind::Equal, CftErrorCode::ExpectedToken)?;
        let value = self.parse_const_literal()?;
        let end = self
            .expect_simple(&TokenKind::Semicolon, CftErrorCode::ExpectedToken)?
            .end;
        Ok(ConstDef {
            name: name.name,
            name_span: name.span,
            ty,
            value,
            annotations,
            span: Span::new(start, end),
        })
    }

    fn parse_const_literal(&mut self) -> Result<ConstLiteral, CftDiagnostics> {
        let token = self.peek().clone();
        match token.kind {
            TokenKind::Int(value) => {
                self.bump();
                Ok(ConstLiteral::Int(value, token.span))
            }
            TokenKind::Float(value) => {
                self.bump();
                Ok(ConstLiteral::Float(value, token.span))
            }
            TokenKind::True => {
                self.bump();
                Ok(ConstLiteral::Bool(true, token.span))
            }
            TokenKind::False => {
                self.bump();
                Ok(ConstLiteral::Bool(false, token.span))
            }
            TokenKind::String(value) => {
                self.bump();
                Ok(ConstLiteral::String(value, token.span))
            }
            TokenKind::Minus => {
                self.bump();
                let next = self.peek().clone();
                let span = Span::new(token.span.start, next.span.end);
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
                        Ok(ConstLiteral::Int(negated, span))
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
                        Ok(ConstLiteral::Int(negated, span))
                    }
                    TokenKind::Float(value) => {
                        self.bump();
                        Ok(ConstLiteral::Float(-value, span))
                    }
                    _ => self.err(CftErrorCode::InvalidConstValue, "expected numeric literal"),
                }
            }
            TokenKind::UIntOverflow(_) => self.err(
                CftErrorCode::InvalidIntLiteral,
                "integer literal out of range",
            ),
            _ => self.err(
                CftErrorCode::InvalidConstValue,
                "const value must be an int, float, bool, or string literal",
            ),
        }
    }

    fn parse_enum(&mut self, annotations: Vec<Annotation>) -> Result<EnumDef, CftDiagnostics> {
        let start = self
            .expect_simple(&TokenKind::Enum, CftErrorCode::UnexpectedToken)?
            .start;
        let name = self.expect_ident()?;
        self.expect_simple(&TokenKind::LBrace, CftErrorCode::ExpectedToken)?;
        let mut variants = Vec::new();
        let mut dangling_annotations = Vec::new();
        let mut pending_annotations = Vec::new();
        while !self.at(&TokenKind::RBrace) {
            if self.at(&TokenKind::Eof) {
                return self.err(CftErrorCode::UnexpectedEof, "unterminated enum definition");
            }
            while self.at(&TokenKind::At) {
                pending_annotations.push(self.parse_annotation()?);
            }
            if self.at(&TokenKind::RBrace) {
                dangling_annotations.append(&mut pending_annotations);
                break;
            }
            let variant_start = self.peek().span.start;
            let variant = self.expect_ident()?;
            let value = if self.eat(&TokenKind::Equal).is_some() {
                Some(self.parse_signed_int()?)
            } else {
                None
            };
            let end = self.prev_span().end;
            variants.push(EnumVariant {
                name: variant.name,
                name_span: variant.span,
                value,
                annotations: std::mem::take(&mut pending_annotations),
                span: Span::new(variant_start, end),
            });
            if self.eat(&TokenKind::Comma).is_none() {
                break;
            }
        }
        let end = self
            .expect_simple(&TokenKind::RBrace, CftErrorCode::ExpectedToken)?
            .end;
        Ok(EnumDef {
            name: name.name,
            name_span: name.span,
            variants,
            annotations,
            dangling_annotations,
            span: Span::new(start, end),
        })
    }

    fn parse_type(&mut self, annotations: Vec<Annotation>) -> Result<TypeDef, CftDiagnostics> {
        let start = self.peek().span.start;
        let mut is_abstract = false;
        let mut abstract_span = None;
        let mut is_sealed = false;
        let mut sealed_span = None;
        loop {
            if let Some(span) = self.eat(&TokenKind::Abstract) {
                is_abstract = true;
                abstract_span = Some(span);
            } else if let Some(span) = self.eat(&TokenKind::Sealed) {
                is_sealed = true;
                sealed_span = Some(span);
            } else {
                break;
            }
        }
        self.expect_simple(&TokenKind::Type, CftErrorCode::ExpectedToken)?;
        let name = self.expect_ident()?;
        let parent = if self.eat(&TokenKind::Colon).is_some() {
            Some(self.expect_ident()?)
        } else {
            None
        };
        self.expect_simple(&TokenKind::LBrace, CftErrorCode::ExpectedToken)?;
        let mut fields = Vec::new();
        let mut check = None;
        let mut dangling_annotations = Vec::new();
        let mut pending_annotations = Vec::new();
        let mut seen_check = false;
        while !self.at(&TokenKind::RBrace) {
            if self.at(&TokenKind::Eof) {
                return self.err(CftErrorCode::UnexpectedEof, "unterminated type definition");
            }
            while self.at(&TokenKind::At) {
                pending_annotations.push(self.parse_annotation()?);
            }
            if self.at(&TokenKind::RBrace) {
                dangling_annotations.append(&mut pending_annotations);
                break;
            }
            if self.at(&TokenKind::Check) {
                if seen_check {
                    return self.err(CftErrorCode::DuplicateCheckBlock, "duplicate check block");
                }
                if !pending_annotations.is_empty() {
                    dangling_annotations.append(&mut pending_annotations);
                }
                seen_check = true;
                check = Some(self.parse_check_block()?);
                continue;
            }
            if seen_check {
                return self.err(
                    CftErrorCode::CheckBlockMustBeLast,
                    "check block must be the last item in a type",
                );
            }
            fields.push(self.parse_field(std::mem::take(&mut pending_annotations))?);
        }
        let end = self
            .expect_simple(&TokenKind::RBrace, CftErrorCode::ExpectedToken)?
            .end;
        Ok(TypeDef {
            name: name.name,
            name_span: name.span,
            is_abstract,
            abstract_span,
            is_sealed,
            sealed_span,
            parent,
            fields,
            check,
            annotations,
            dangling_annotations,
            span: Span::new(start, end),
        })
    }

    fn parse_field(&mut self, annotations: Vec<Annotation>) -> Result<FieldDef, CftDiagnostics> {
        let start = self.peek().span.start;
        let name = self.expect_ident()?;
        self.expect_simple(&TokenKind::Colon, CftErrorCode::ExpectedToken)?;
        let ty = self.parse_type_ref()?;
        let default = if self.eat(&TokenKind::Equal).is_some() {
            Some(self.parse_default_expr()?)
        } else {
            None
        };
        let end = self
            .expect_simple(&TokenKind::Semicolon, CftErrorCode::ExpectedToken)?
            .end;
        Ok(FieldDef {
            name: name.name,
            name_span: name.span,
            ty,
            default,
            annotations,
            span: Span::new(start, end),
        })
    }

    fn parse_type_ref(&mut self) -> Result<TypeRef, CftDiagnostics> {
        let mut ty = if let Some(start) = self.eat(&TokenKind::LBracket) {
            let inner = self.parse_type_ref()?;
            let end = self
                .expect_simple(&TokenKind::RBracket, CftErrorCode::ExpectedToken)?
                .end;
            TypeRef {
                span: Span::new(start.start, end),
                kind: TypeRefKind::Array(Box::new(inner)),
            }
        } else if let Some(start) = self.eat(&TokenKind::LBrace) {
            let key = self.parse_type_ref()?;
            self.expect_simple(&TokenKind::Colon, CftErrorCode::ExpectedToken)?;
            let value = self.parse_type_ref()?;
            let end = self
                .expect_simple(&TokenKind::RBrace, CftErrorCode::ExpectedToken)?
                .end;
            TypeRef {
                span: Span::new(start.start, end),
                kind: TypeRefKind::Dict(Box::new(key), Box::new(value)),
            }
        } else {
            let name = self.expect_ident()?;
            let kind = match name.name.as_str() {
                "int" => TypeRefKind::Int,
                "float" => TypeRefKind::Float,
                "bool" => TypeRefKind::Bool,
                "string" => TypeRefKind::String,
                _ => TypeRefKind::Named(name.name),
            };
            TypeRef {
                kind,
                span: name.span,
            }
        };
        if let Some(question) = self.eat(&TokenKind::Question) {
            ty = TypeRef {
                span: ty.span.join(question),
                kind: TypeRefKind::Nullable(Box::new(ty)),
            };
        }
        Ok(ty)
    }

    fn parse_default_expr(&mut self) -> Result<DefaultExpr, CftDiagnostics> {
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

    fn parse_check_block(&mut self) -> Result<CheckBlock, CftDiagnostics> {
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
        let mut expr = self.parse_bitxor_expr()?;
        while self.eat(&TokenKind::Pipe).is_some() {
            let rhs = self.parse_bitxor_expr()?;
            expr = bin_expr(BinOp::BitOr, expr, rhs);
        }
        Ok(expr)
    }

    fn parse_bitxor_expr(&mut self) -> Result<CheckExpr, CftDiagnostics> {
        let mut expr = self.parse_bitand_expr()?;
        while self.eat(&TokenKind::Caret).is_some() {
            let rhs = self.parse_bitand_expr()?;
            expr = bin_expr(BinOp::BitXor, expr, rhs);
        }
        Ok(expr)
    }

    fn parse_bitand_expr(&mut self) -> Result<CheckExpr, CftDiagnostics> {
        let mut expr = self.parse_add_expr()?;
        while self.eat(&TokenKind::Amp).is_some() {
            let rhs = self.parse_add_expr()?;
            expr = bin_expr(BinOp::BitAnd, expr, rhs);
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
            // Special case: `- <UIntOverflow>` is the only legal place for a
            // magnitude that exceeds i64::MAX (e.g. `-9223372036854775808`).
            // Fold it into a single Int expression here so the AST never has
            // to carry the unsigned magnitude.
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
                let span = Span::new(expr.span.start, name.span.end);
                expr = CheckExpr {
                    span,
                    kind: CheckExprKind::Field {
                        expr: Box::new(expr),
                        name,
                    },
                };
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

    fn parse_signed_int(&mut self) -> Result<SignedInt, CftDiagnostics> {
        let sign_span = self.eat(&TokenKind::Minus);
        let token = self.peek().clone();
        let span = sign_span.map_or(token.span, |span| span.join(token.span));
        match token.kind {
            TokenKind::Int(value) => {
                self.bump();
                let value = if sign_span.is_some() {
                    let Some(negated) = value.checked_neg() else {
                        return self.err_at(
                            CftErrorCode::InvalidIntLiteral,
                            span,
                            "negated integer literal overflowed",
                        );
                    };
                    negated
                } else {
                    value
                };
                Ok(SignedInt { value, span })
            }
            TokenKind::UIntOverflow(value) => {
                self.bump();
                if sign_span.is_some() {
                    let Some(negated) = negate_u64_to_i64(value) else {
                        return self.err_at(
                            CftErrorCode::InvalidIntLiteral,
                            span,
                            "integer literal out of range",
                        );
                    };
                    Ok(SignedInt {
                        value: negated,
                        span,
                    })
                } else {
                    self.err_at(
                        CftErrorCode::InvalidIntLiteral,
                        span,
                        "integer literal out of range",
                    )
                }
            }
            _ => self.err(CftErrorCode::ExpectedToken, "expected integer literal"),
        }
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

    fn expect_ident(&mut self) -> Result<NameRef, CftDiagnostics> {
        self.expect_ident_with_code(CftErrorCode::ExpectedIdentifier)
    }

    fn expect_ident_with_code(&mut self, code: CftErrorCode) -> Result<NameRef, CftDiagnostics> {
        let token = self.peek().clone();
        match token.kind {
            TokenKind::Ident(name) => {
                self.bump();
                Ok(NameRef {
                    name,
                    span: token.span,
                })
            }
            _ => self.err(code, "expected identifier"),
        }
    }

    fn expect_simple(
        &mut self,
        kind: &TokenKind,
        code: CftErrorCode,
    ) -> Result<Span, CftDiagnostics> {
        if self.at(kind) {
            Ok(self.bump().span)
        } else {
            self.err(code, format!("expected `{}`", token_name(kind)))
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

    fn err<T>(&self, code: CftErrorCode, message: impl Into<String>) -> Result<T, CftDiagnostics> {
        self.err_at(code, self.peek().span, message)
    }

    fn err_at<T>(
        &self,
        code: CftErrorCode,
        span: Span,
        message: impl Into<String>,
    ) -> Result<T, CftDiagnostics> {
        Err(CftDiagnostics::one(CftDiagnostic::error(
            code,
            self.module.clone(),
            span,
            message,
        )))
    }
}

/// Folds `-magnitude` where `magnitude > i64::MAX` into the equivalent `i64`.
/// Only `2^63` (i.e. `i64::MIN.unsigned_abs()`) is representable; any larger
/// magnitude is out of range and returns `None`.
fn negate_u64_to_i64(magnitude: u64) -> Option<i64> {
    const I64_MIN_MAGNITUDE: u64 = i64::MIN.unsigned_abs();
    if magnitude == I64_MIN_MAGNITUDE {
        Some(i64::MIN)
    } else {
        None
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
        TokenKind::Question => "?",
        TokenKind::In => "in",
        _ => "token",
    }
}
