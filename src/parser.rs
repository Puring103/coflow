use crate::ast::{
    CheckBlock, DataDef, EnumDef, EnumVariant, Expr, ExprKind, FieldDef, Item, ModuleAst,
    ObjectField, PathSegment, TypeDef, TypeName, TypeRef, UseDecl,
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
        while !self.at(&TokenKind::RBrace) {
            if self.at(&TokenKind::Check) {
                self.parse_check_block()?;
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
        let mut depth = 1;
        while depth > 0 {
            let token = self.peek().clone();
            match token.kind {
                TokenKind::LBrace => {
                    depth += 1;
                    self.bump();
                }
                TokenKind::RBrace => {
                    depth -= 1;
                    self.bump();
                    if depth == 0 {
                        return Ok(CheckBlock {
                            span: Span::new(start, token.span.end),
                        });
                    }
                }
                TokenKind::Eof => return self.err("unterminated check block"),
                _ => {
                    self.bump();
                }
            }
        }
        self.err("unterminated check block")
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
