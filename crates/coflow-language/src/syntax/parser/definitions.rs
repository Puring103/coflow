use super::{Parsed, Parser};
use crate::diagnostics::{CftDiagnostics, CftErrorCode};
use crate::limits::StructureKind;
use crate::syntax::ast::{
    Annotation, ConstDef, EnumDef, EnumVariant, FieldDef, Item, TypeAliasDef, TypeDef, TypeRef,
    TypeRefKind,
};
use crate::syntax::lexer::{Token, TokenKind};
use crate::syntax::Span;

impl Parser<'_> {
    pub(super) fn parse_const(
        &mut self,
        annotations: Vec<Annotation>,
    ) -> Result<ConstDef, CftDiagnostics> {
        let start = self
            .expect_simple(&TokenKind::Const, CftErrorCode::UnexpectedToken)?
            .start;
        let name = self.expect_ident()?;
        let ty = if self.eat(&TokenKind::Colon).is_some() {
            Some(self.parse_value_type()?.value)
        } else {
            None
        };
        self.expect_simple(&TokenKind::Equal, CftErrorCode::ExpectedToken)?;
        let value = self.parse_default_expr()?.value;
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

    pub(super) fn parse_enum(
        &mut self,
        annotations: Vec<Annotation>,
    ) -> Result<EnumDef, CftDiagnostics> {
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
            self.charge_nodes(StructureKind::SyntaxAst, Span::new(variant_start, end), 1)?;
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

    pub(super) fn parse_type(
        &mut self,
        annotations: Vec<Annotation>,
    ) -> Result<Item, CftDiagnostics> {
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
        if self.eat(&TokenKind::Equal).is_some() {
            if let Some(span) = abstract_span.or(sealed_span) {
                return self.err_at(
                    CftErrorCode::InvalidTopLevelItem,
                    span,
                    "type aliases cannot have abstract or sealed modifiers",
                );
            }
            let target = self.parse_value_type()?.value;
            let end = self
                .expect_simple(&TokenKind::Semicolon, CftErrorCode::ExpectedToken)?
                .end;
            return Ok(Item::TypeAlias(TypeAliasDef {
                name: name.name,
                name_span: name.span,
                target,
                annotations,
                span: Span::new(start, end),
            }));
        }
        let parent = if self.eat(&TokenKind::Colon).is_some() {
            Some(self.expect_qualified_name_ref()?)
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
        Ok(Item::Type(TypeDef {
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
        }))
    }

    fn parse_field(&mut self, annotations: Vec<Annotation>) -> Result<FieldDef, CftDiagnostics> {
        let start = self.peek().span.start;
        let name = self.expect_ident()?;
        self.expect_simple(&TokenKind::Colon, CftErrorCode::ExpectedToken)?;
        let ty = self.parse_value_type()?.value;
        let default = if self.eat(&TokenKind::Equal).is_some() {
            Some(self.parse_default_expr()?.value)
        } else {
            None
        };
        let end = self
            .expect_simple(&TokenKind::Semicolon, CftErrorCode::ExpectedToken)?
            .end;
        let field = FieldDef {
            name: name.name,
            name_span: name.span,
            ty,
            default,
            annotations,
            span: Span::new(start, end),
        };
        self.charge_nodes(StructureKind::SyntaxAst, field.span, 1)?;
        Ok(field)
    }

    pub(super) fn parse_value_type(&mut self) -> Result<Parsed<TypeRef>, CftDiagnostics> {
        self.parse_value_type_primary()
    }

    fn parse_value_type_primary(&mut self) -> Result<Parsed<TypeRef>, CftDiagnostics> {
        if let Some(start) = self.eat(&TokenKind::LParen) {
            let end = self
                .expect_simple(&TokenKind::RParen, CftErrorCode::ExpectedToken)?
                .end;
            return self.node(StructureKind::TypeRef, start, [], || TypeRef {
                span: Span::new(start.start, end),
                kind: TypeRefKind::Unit,
            });
        }
        if self.peek_ident_is("Option") {
            let start = self.expect_ident()?.span;
            self.expect_simple(&TokenKind::Less, CftErrorCode::ExpectedToken)?;
            let inner = self.parse_value_type()?;
            let end = self.expect_type_close()?.end;
            let depth = inner.depth;
            return self.node(StructureKind::TypeRef, start, [depth], || TypeRef {
                span: Span::new(start.start, end),
                kind: TypeRefKind::Option(Box::new(inner.value)),
            });
        }
        if self.peek_ident_is("Result") {
            let start = self.expect_ident()?.span;
            self.expect_simple(&TokenKind::Less, CftErrorCode::ExpectedToken)?;
            let value = self.parse_value_type()?;
            self.expect_simple(&TokenKind::Comma, CftErrorCode::ExpectedToken)?;
            let error = self.parse_value_type()?;
            let end = self.expect_type_close()?.end;
            let depths = [value.depth, error.depth];
            return self.node(StructureKind::TypeRef, start, depths, || TypeRef {
                span: Span::new(start.start, end),
                kind: TypeRefKind::Result(Box::new(value.value), Box::new(error.value)),
            });
        }
        if self.peek_ident_is("fn") {
            let start = self.expect_ident()?.span;
            self.expect_simple(&TokenKind::LParen, CftErrorCode::ExpectedToken)?;
            let mut parameters = Vec::new();
            let mut depths = Vec::new();
            while !self.at(&TokenKind::RParen) {
                let parameter = self.parse_value_type()?;
                depths.push(parameter.depth);
                parameters.push(parameter.value);
                if self.eat(&TokenKind::Comma).is_none() {
                    break;
                }
            }
            self.expect_simple(&TokenKind::RParen, CftErrorCode::ExpectedToken)?;
            self.expect_simple(&TokenKind::Arrow, CftErrorCode::ExpectedToken)?;
            let result = self.parse_value_type()?;
            let end = result.value.span.end;
            depths.push(result.depth);
            return self.node(StructureKind::TypeRef, start, depths, || TypeRef {
                span: Span::new(start.start, end),
                kind: TypeRefKind::Function(parameters, Box::new(result.value)),
            });
        }
        if let Some(start) = self.eat(&TokenKind::Amp) {
            let inner = self.nested(StructureKind::TypeRef, start, |parser| {
                parser.parse_value_type_primary()
            })?;
            let span = Span::new(start.start, inner.value.span.end);
            let depth = inner.depth;
            return self.node(StructureKind::TypeRef, start, [depth], || TypeRef {
                span,
                kind: TypeRefKind::Ref(Box::new(inner.value)),
            });
        }
        if let Some(start) = self.eat(&TokenKind::LBracket) {
            let inner = self.nested(StructureKind::TypeRef, start, |parser| {
                parser.parse_value_type()
            })?;
            let end = self
                .expect_simple(&TokenKind::RBracket, CftErrorCode::ExpectedToken)?
                .end;
            let depth = inner.depth;
            self.node(StructureKind::TypeRef, start, [depth], || TypeRef {
                span: Span::new(start.start, end),
                kind: TypeRefKind::Array(Box::new(inner.value)),
            })
        } else if let Some(start) = self.eat(&TokenKind::LBrace) {
            let key = self.nested(StructureKind::TypeRef, start, |parser| {
                parser.parse_value_type()
            })?;
            self.expect_simple(&TokenKind::Colon, CftErrorCode::ExpectedToken)?;
            let value_span = self.peek().span;
            let value = self.nested(StructureKind::TypeRef, value_span, |parser| {
                parser.parse_value_type()
            })?;
            let end = self
                .expect_simple(&TokenKind::RBrace, CftErrorCode::ExpectedToken)?
                .end;
            let depths = [key.depth, value.depth];
            self.node(StructureKind::TypeRef, start, depths, || TypeRef {
                span: Span::new(start.start, end),
                kind: TypeRefKind::Dict(Box::new(key.value), Box::new(value.value)),
            })
        } else {
            let name = self.expect_qualified_name_ref()?;
            let kind = match name.name.as_str() {
                "int" => TypeRefKind::Int,
                "float" => TypeRefKind::Float,
                "bool" => TypeRefKind::Bool,
                "string" => TypeRefKind::String,
                _ => TypeRefKind::Named(name.name),
            };
            self.node(StructureKind::TypeRef, name.span, [], || TypeRef {
                kind,
                span: name.span,
            })
        }
    }

    fn expect_type_close(&mut self) -> Result<Span, CftDiagnostics> {
        let token = self.peek().clone();
        match token.kind {
            TokenKind::Greater => Ok(self.bump().span),
            TokenKind::GreaterGreater => {
                let split = token.span.start + 1;
                self.tokens[self.pos] = Token {
                    kind: TokenKind::Greater,
                    span: Span::new(split, token.span.end),
                };
                Ok(Span::new(token.span.start, split))
            }
            TokenKind::GreaterEq => {
                let split = token.span.start + 1;
                self.tokens[self.pos] = Token {
                    kind: TokenKind::Equal,
                    span: Span::new(split, token.span.end),
                };
                Ok(Span::new(token.span.start, split))
            }
            _ => self.err(CftErrorCode::ExpectedToken, "expected `>`"),
        }
    }
}
