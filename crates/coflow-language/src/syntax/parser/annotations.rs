use super::Parser;
use crate::diagnostics::{CftDiagnostics, CftErrorCode};
use crate::limits::StructureKind;
use crate::source::Span;
use crate::syntax::ast::{Annotation, AnnotationArg};
use crate::syntax::lexer::TokenKind;

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
        let span = self.peek().span;
        match &mut self.tokens[self.pos].kind {
            TokenKind::Ident(_) => self
                .expect_ident_with_code(CftErrorCode::ExpectedIdentifier)
                .map(AnnotationArg::Name),
            TokenKind::String(value) => {
                let value = std::mem::take(value);
                self.bump();
                Ok(AnnotationArg::String(value, span))
            }
            TokenKind::Int(value) => {
                let value = *value;
                self.bump();
                Ok(AnnotationArg::Int(value, span))
            }
            TokenKind::Float(value) => {
                let value = *value;
                self.bump();
                Ok(AnnotationArg::Float(value, span))
            }
            TokenKind::True => {
                self.bump();
                Ok(AnnotationArg::Bool(true, span))
            }
            TokenKind::False => {
                self.bump();
                Ok(AnnotationArg::Bool(false, span))
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
            let span = self.peek().span;
            if let TokenKind::String(value) = &mut self.tokens[self.pos].kind {
                let value = std::mem::take(value);
                self.bump();
                return Ok(AnnotationArg::String(value, span));
            }
            return self.err_at(
                CftErrorCode::InvalidAnnotationSyntax,
                span,
                "expected string literal for @localized bucket",
            );
        }
        self.parse_annotation_arg()
    }
}
