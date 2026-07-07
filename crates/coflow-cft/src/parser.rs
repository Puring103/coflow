mod annotations;
mod check;
mod defaults;
mod tokens;

use self::tokens::{reserved_keyword_name, token_name};
use crate::ast::{
    Annotation, ConstDef, ConstLiteral, EnumDef, EnumVariant, FieldDef, Item, ModuleAst, NameRef,
    SignedInt, TypeDef, TypeRef, TypeRefKind,
};
use crate::container::ModuleId;
use crate::error::{CftDiagnostic, CftDiagnostics, CftErrorCode};
use crate::lexer::{lex, Token, TokenKind};
use crate::span::Span;

/// Parses one CFT module into its AST.
///
/// # Errors
///
/// Returns diagnostics when lexing fails or when tokens do not match the CFT
/// grammar.
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
            if self.at(&TokenKind::Check) && self.next_at(&TokenKind::LBrace) {
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
        let mut ty = self.parse_type_ref_primary()?;
        if let Some(question) = self.eat(&TokenKind::Question) {
            ty = TypeRef {
                span: ty.span.join(question),
                kind: TypeRefKind::Nullable(Box::new(ty)),
            };
        }
        Ok(ty)
    }

    fn parse_type_ref_primary(&mut self) -> Result<TypeRef, CftDiagnostics> {
        if let Some(start) = self.eat(&TokenKind::Amp) {
            let inner = self.parse_type_ref_primary()?;
            return Ok(TypeRef {
                span: Span::new(start.start, inner.span.end),
                kind: TypeRefKind::Ref(Box::new(inner)),
            });
        }
        if let Some(start) = self.eat(&TokenKind::LBracket) {
            let inner = self.parse_type_ref()?;
            let end = self
                .expect_simple(&TokenKind::RBracket, CftErrorCode::ExpectedToken)?
                .end;
            Ok(TypeRef {
                span: Span::new(start.start, end),
                kind: TypeRefKind::Array(Box::new(inner)),
            })
        } else if let Some(start) = self.eat(&TokenKind::LBrace) {
            let key = self.parse_type_ref()?;
            self.expect_simple(&TokenKind::Colon, CftErrorCode::ExpectedToken)?;
            let value = self.parse_type_ref()?;
            let end = self
                .expect_simple(&TokenKind::RBrace, CftErrorCode::ExpectedToken)?
                .end;
            Ok(TypeRef {
                span: Span::new(start.start, end),
                kind: TypeRefKind::Dict(Box::new(key), Box::new(value)),
            })
        } else {
            let name = self.expect_ident()?;
            let kind = match name.name.as_str() {
                "int" => TypeRefKind::Int,
                "float" => TypeRefKind::Float,
                "bool" => TypeRefKind::Bool,
                "string" => TypeRefKind::String,
                _ => TypeRefKind::Named(name.name),
            };
            Ok(TypeRef {
                kind,
                span: name.span,
            })
        }
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

    pub(super) fn expect_ident(&mut self) -> Result<NameRef, CftDiagnostics> {
        self.expect_name(CftErrorCode::ExpectedIdentifier, true)
    }

    pub(super) fn expect_ident_with_code(
        &mut self,
        code: CftErrorCode,
    ) -> Result<NameRef, CftDiagnostics> {
        self.expect_name(code, false)
    }

    fn expect_name(
        &mut self,
        code: CftErrorCode,
        allow_reserved_keywords: bool,
    ) -> Result<NameRef, CftDiagnostics> {
        let token = self.peek().clone();
        match token.kind {
            TokenKind::Ident(name) => {
                self.bump();
                Ok(NameRef {
                    name,
                    span: token.span,
                })
            }
            _ if allow_reserved_keywords => {
                if let Some(name) = reserved_keyword_name(&token.kind) {
                    self.bump();
                    Ok(NameRef {
                        name: name.to_string(),
                        span: token.span,
                    })
                } else {
                    self.err(code, "expected identifier")
                }
            }
            _ => self.err(code, "expected identifier"),
        }
    }

    pub(super) fn expect_simple(
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

    pub(super) fn eat(&mut self, kind: &TokenKind) -> Option<Span> {
        if self.at(kind) {
            Some(self.bump().span)
        } else {
            None
        }
    }

    pub(super) fn at(&self, kind: &TokenKind) -> bool {
        std::mem::discriminant(&self.peek().kind) == std::mem::discriminant(kind)
    }

    pub(super) fn next_at(&self, kind: &TokenKind) -> bool {
        self.tokens.get(self.pos + 1).is_some_and(|token| {
            std::mem::discriminant(&token.kind) == std::mem::discriminant(kind)
        })
    }

    pub(super) fn peek_ident_is(&self, name: &str) -> bool {
        matches!(&self.peek().kind, TokenKind::Ident(value) if value == name)
    }

    pub(super) fn bump(&mut self) -> Token {
        let token = self.tokens[self.pos].clone();
        self.pos += 1;
        token
    }

    pub(super) fn peek(&self) -> &Token {
        &self.tokens[self.pos]
    }

    fn prev_span(&self) -> Span {
        self.tokens[self.pos - 1].span
    }

    pub(super) fn err<T>(
        &self,
        code: CftErrorCode,
        message: impl Into<String>,
    ) -> Result<T, CftDiagnostics> {
        self.err_at(code, self.peek().span, message)
    }

    pub(super) fn err_at<T>(
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
