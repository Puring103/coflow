use super::Parser;
use crate::diagnostics::{CftDiagnostics, CftErrorCode};
use crate::source::Span;
use crate::syntax::ast::CheckBlock;
use crate::syntax::lexer::TokenKind;

impl Parser<'_> {
    pub(super) fn parse_check_block(&mut self) -> Result<CheckBlock, CftDiagnostics> {
        let start = self
            .expect_simple(&TokenKind::Check, CftErrorCode::UnexpectedToken)?
            .start;
        if !self.at(&TokenKind::LBrace) {
            self.expect_ident()?;
        }
        self.parse_check_block_after_keyword(start)
    }

    pub(super) fn parse_top_level_check(
        &mut self,
        annotations: Vec<crate::syntax::ast::Annotation>,
    ) -> Result<crate::syntax::ast::TopLevelCheckDef, CftDiagnostics> {
        let start = self
            .expect_simple(&TokenKind::Check, CftErrorCode::UnexpectedToken)?
            .start;
        let name = self.expect_ident()?;
        let block = self.parse_check_block_after_keyword(start)?;
        Ok(crate::syntax::ast::TopLevelCheckDef {
            name: name.name,
            name_span: name.span,
            span: block.span,
            block,
            annotations,
        })
    }

    fn parse_check_block_after_keyword(
        &mut self,
        start: usize,
    ) -> Result<CheckBlock, CftDiagnostics> {
        self.expect_simple(&TokenKind::LBrace, CftErrorCode::ExpectedToken)?;
        let mut depth = 1usize;
        let mut end = start;
        while depth > 0 {
            let token = self.bump();
            match token.kind {
                TokenKind::LBrace => depth += 1,
                TokenKind::RBrace => depth -= 1,
                TokenKind::Eof => {
                    return self.err(CftErrorCode::UnexpectedEof, "unterminated check")
                }
                _ => {}
            }
            end = token.span.end;
        }
        Ok(CheckBlock {
            source: self.source[start..end].to_string(),
            span: Span::new(start, end),
        })
    }
}
