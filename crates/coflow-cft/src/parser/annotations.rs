use super::Parser;
use crate::ast::{Annotation, AnnotationArg};
use crate::error::{CftDiagnostics, CftErrorCode};
use crate::lexer::TokenKind;
use crate::span::Span;
use coflow_structure::StructureKind;

impl Parser<'_> {
    pub(super) fn parse_annotation(&mut self) -> Result<Annotation, CftDiagnostics> {
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
                args.push(self.parse_annotation_arg_for(&name.name)?);
                if self.eat(&TokenKind::Comma).is_none() {
                    break;
                }
            }
            end = self
                .expect_simple(&TokenKind::RParen, CftErrorCode::InvalidAnnotationSyntax)?
                .end;
        }
        let annotation = Annotation {
            name: name.name,
            name_span: name.span,
            args,
            span: Span::new(start, end),
        };
        let nodes = u64::try_from(annotation.args.len())
            .unwrap_or(u64::MAX)
            .saturating_add(1);
        self.charge_nodes(StructureKind::SyntaxAst, annotation.span, nodes)?;
        Ok(annotation)
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

    fn parse_annotation_arg_for(
        &mut self,
        annotation_name: &str,
    ) -> Result<AnnotationArg, CftDiagnostics> {
        if annotation_name == "localized"
            && self.peek_ident_is("bucket")
            && self.next_at(&TokenKind::Equal)
        {
            let _bucket = self.expect_ident_with_code(CftErrorCode::InvalidAnnotationSyntax)?;
            self.expect_simple(&TokenKind::Equal, CftErrorCode::InvalidAnnotationSyntax)?;
            let token = self.peek().clone();
            if let TokenKind::String(value) = token.kind {
                self.bump();
                return Ok(AnnotationArg::String(value, token.span));
            }
            return self.err_at(
                CftErrorCode::InvalidAnnotationSyntax,
                token.span,
                "expected string literal for @localized bucket",
            );
        }
        self.parse_annotation_arg()
    }
}
