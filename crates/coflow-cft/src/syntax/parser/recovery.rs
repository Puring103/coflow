use super::Parser;
use crate::diagnostics::{CftDiagnostics, CftErrorCode};
use crate::syntax::ast::{Annotation, Item, ModuleAst};
use crate::syntax::lexer::TokenKind;
use coflow_structure::StructureKind;

impl Parser<'_> {
    pub(super) fn parse_module(&mut self) -> Result<ModuleAst, CftDiagnostics> {
        let mut items = Vec::new();
        let mut pending_annotations = Vec::new();
        let mut diagnostics = Vec::new();
        while !self.at(&TokenKind::Eof) {
            let declaration_start = self.pos;
            match self.parse_annotated_item(&mut pending_annotations) {
                Ok(Some(item)) => {
                    if let Err(mut error) =
                        self.charge_nodes(StructureKind::SyntaxAst, item.span(), 1)
                    {
                        diagnostics.append(&mut error.diagnostics);
                        return Err(CftDiagnostics::new(diagnostics));
                    }
                    items.push(item);
                }
                Ok(None) => break,
                Err(mut error) => {
                    let limit_exceeded = error.diagnostics.iter().any(|diagnostic| {
                        diagnostic.code == CftErrorCode::SyntaxStructureLimitExceeded
                    });
                    diagnostics.append(&mut error.diagnostics);
                    if limit_exceeded {
                        return Err(CftDiagnostics::new(diagnostics));
                    }
                    pending_annotations.clear();
                    self.recover_declaration(declaration_start);
                }
            }
        }
        if !diagnostics.is_empty() {
            return Err(CftDiagnostics::new(diagnostics));
        }
        Ok(ModuleAst {
            items,
            dangling_annotations: pending_annotations,
        })
    }

    fn parse_annotated_item(
        &mut self,
        pending_annotations: &mut Vec<Annotation>,
    ) -> Result<Option<Item>, CftDiagnostics> {
        while self.at(&TokenKind::At) {
            pending_annotations.push(self.parse_annotation()?);
        }
        if self.at(&TokenKind::Eof) {
            return Ok(None);
        }
        let annotations = std::mem::take(pending_annotations);
        if self.at(&TokenKind::Const) {
            self.parse_const(annotations).map(Item::Const).map(Some)
        } else if self.at(&TokenKind::Enum) {
            self.parse_enum(annotations).map(Item::Enum).map(Some)
        } else if self.at(&TokenKind::Type)
            || self.at(&TokenKind::Abstract)
            || self.at(&TokenKind::Sealed)
        {
            self.parse_type(annotations).map(Item::Type).map(Some)
        } else {
            self.err(
                CftErrorCode::InvalidTopLevelItem,
                "top level items must be const, enum, or type definitions",
            )
        }
    }

    fn recover_declaration(&mut self, declaration_start: usize) {
        let mut brace_depth =
            self.tokens[declaration_start..self.pos]
                .iter()
                .fold(0_u64, |depth, token| match token.kind {
                    TokenKind::LBrace => depth.saturating_add(1),
                    TokenKind::RBrace => depth.saturating_sub(1),
                    _ => depth,
                });
        while !self.at(&TokenKind::Eof) {
            if brace_depth == 0 && self.pos > declaration_start && self.at_declaration_start() {
                return;
            }
            match self.bump().kind {
                TokenKind::LBrace => brace_depth = brace_depth.saturating_add(1),
                TokenKind::RBrace => brace_depth = brace_depth.saturating_sub(1),
                _ => {}
            }
        }
    }

    fn at_declaration_start(&self) -> bool {
        self.at(&TokenKind::At)
            || self.at(&TokenKind::Const)
            || self.at(&TokenKind::Enum)
            || self.at(&TokenKind::Type)
            || self.at(&TokenKind::Abstract)
            || self.at(&TokenKind::Sealed)
    }
}
