use super::{Parsed, Parser};
use crate::diagnostics::{CftDiagnostics, CftErrorCode};
use crate::limits::StructureKind;
use crate::source::Span;
use crate::syntax::ast::{
    Annotation, ConstDef, EnumDef, EnumVariant, FieldDef, Item, TypeAliasDef, TypeDef, TypeRef,
    TypeRefKind,
};
use crate::syntax::lexer::TokenKind;

impl Parser<'_> {
    pub(super) fn parse_const(
        &mut self,
        annotations: Vec<Annotation>,
    ) -> Result<ConstDef, CftDiagnostics> {
        let start = self
            .expect_simple(&TokenKind::Const, CftErrorCode::UnexpectedToken)?
            .start;
        let name = self.expect_ident()?;
        self.expect_simple(&TokenKind::Colon, CftErrorCode::ExpectedToken)?;
        let ty = Some(self.parse_value_type()?.value);
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
        use crate::syntax::ast::TypeKind;
        let kind = match self.bump().kind {
            TokenKind::Table => Some(TypeKind::Table),
            TokenKind::Singleton => Some(TypeKind::Singleton),
            TokenKind::Data => Some(TypeKind::Data),
            TokenKind::Type => None,
            _ => {
                return self.err(
                    CftErrorCode::ExpectedToken,
                    "expected table, singleton, data or type",
                )
            }
        };
        let name = self.expect_ident()?;
        if self.eat(&TokenKind::Equal).is_some() {
            if kind.is_some() {
                return self.err(CftErrorCode::InvalidTopLevelItem, "aliases must use type");
            }
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
        let Some(kind) = kind else {
            return self.err(
                CftErrorCode::InvalidTopLevelItem,
                "object declarations use table, singleton or data",
            );
        };
        let parent = if self.eat(&TokenKind::Colon).is_some() {
            let path = self.expect_name_path()?;
            Some(crate::syntax::ast::NameRef {
                name: path.canonical(),
                span: path.span,
            })
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
                if kind == TypeKind::Data {
                    return self.err(
                        CftErrorCode::InvalidTopLevelItem,
                        "data cannot declare check",
                    );
                }
                if !pending_annotations.is_empty() {
                    dangling_annotations.append(&mut pending_annotations);
                }
                seen_check = true;
                let next = self.parse_check_block()?;
                if let Some(previous) = &mut check {
                    let previous: &mut crate::syntax::ast::CheckBlock = previous;
                    previous.source.push('\n');
                    previous.source.push_str(&next.source);
                    previous.span = previous.span.join(next.span);
                } else {
                    check = Some(next);
                }
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
            kind,
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
            if self.eat(&TokenKind::Greater).is_some() {
                match &ty.kind {
                    TypeRefKind::Function(parameters, _)
                        if parameters.iter().any(|parameter| parameter.name.is_none()) =>
                    {
                        return self.err(
                            CftErrorCode::InvalidDefaultExpression,
                            "function implementation requires named parameters",
                        );
                    }
                    TypeRefKind::Function(..) | TypeRefKind::Named(_) => {}
                    _ => {
                        return self.err(
                            CftErrorCode::InvalidDefaultExpression,
                            "=> requires a function field",
                        )
                    }
                }
                let body = self.capture_function_body()?;
                let source = format!(
                    "{} {}",
                    &self.source[ty.span.start..ty.span.end],
                    &self.source[body.start..body.end]
                );
                Some(crate::syntax::ast::DefaultExpr {
                    kind: crate::syntax::ast::DefaultExprKind::Function {
                        signature: ty.clone(),
                        source,
                    },
                    span: ty.span.join(body),
                })
            } else {
                Some(self.parse_default_expr()?.value)
            }
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
        let mut value = self.parse_value_type_primary()?;
        if let Some(end) = self.eat(&TokenKind::Question) {
            if matches!(value.value.kind, TypeRefKind::Option(_)) {
                return self.err(
                    CftErrorCode::ExpectedToken,
                    "nested optional types are not supported",
                );
            }
            let span = value.value.span.join(end);
            value = self.node(StructureKind::TypeRef, span, [value.depth], || TypeRef {
                kind: TypeRefKind::Option(Box::new(value.value)),
                span,
            })?;
        }
        Ok(value)
    }

    // 类型语法的所有首 token 分支集中处理，便于核对递归深度预算。
    #[allow(clippy::too_many_lines)]
    fn parse_value_type_primary(&mut self) -> Result<Parsed<TypeRef>, CftDiagnostics> {
        if let Some(start) = self.eat(&TokenKind::LParen) {
            if !self.at(&TokenKind::RParen) {
                // 括号区分可选函数与返回可选值的函数，同时参与结构深度限制。
                let mut inner = self.nested(StructureKind::TypeRef, start, |parser| {
                    parser.parse_value_type()
                })?;
                let end = self.expect_simple(&TokenKind::RParen, CftErrorCode::ExpectedToken)?;
                inner.value.span = start.join(end);
                return Ok(inner);
            }
            let end = self
                .expect_simple(&TokenKind::RParen, CftErrorCode::ExpectedToken)?
                .end;
            return self.node(StructureKind::TypeRef, start, [], || TypeRef {
                span: Span::new(start.start, end),
                kind: TypeRefKind::Unit,
            });
        }
        if self.peek_ident_is("fn") {
            use crate::syntax::ast::FunctionParameterRef;

            let start = self.expect_ident()?.span;
            self.expect_simple(&TokenKind::LParen, CftErrorCode::ExpectedToken)?;
            let mut parameters = Vec::new();
            let mut depths = Vec::new();
            while !self.at(&TokenKind::RParen) {
                let name = if self.next_at(&TokenKind::Colon) {
                    let name = self.expect_ident_with_code(CftErrorCode::ExpectedIdentifier)?;
                    self.expect_simple(&TokenKind::Colon, CftErrorCode::ExpectedToken)?;
                    Some(name)
                } else {
                    None
                };
                let value_type = self.parse_value_type()?;
                depths.push(value_type.depth);
                parameters.push(FunctionParameterRef {
                    name,
                    value_type: value_type.value,
                });
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
            let path = self.expect_name_path()?;
            let name = crate::syntax::ast::NameRef {
                name: path.canonical(),
                span: path.span,
            };
            let kind = match name.name.as_str() {
                "int" => TypeRefKind::Int,
                "float" => TypeRefKind::Float,
                "bool" => TypeRefKind::Bool,
                "string" => TypeRefKind::String,
                "fstring" => TypeRefKind::FString,
                _ => TypeRefKind::Named(name.name),
            };
            self.node(StructureKind::TypeRef, name.span, [], || TypeRef {
                kind,
                span: name.span,
            })
        }
    }
}
