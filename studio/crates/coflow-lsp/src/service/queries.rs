//! 编辑器与独立 LSP 共用的类型化语义查询。
use super::{
    DocumentSymbol, Hover, LanguageCompletion, LanguageTextEdit, Location, SemanticTokens,
};
use crate::{
    byte_offset_from_position, cfd, cfd_definition, completion_items, definitions_at,
    document_symbols, format_cfd, format_cft, formatting_edits, hover_at, semantic_token_data,
    LspPosition, LspRequestDocument, LspValidationCore,
};

impl LspValidationCore {
    pub(crate) fn completion(&self, uri: &str, position: LspPosition) -> Vec<LanguageCompletion> {
        let result = match self.request_document(uri) {
            LspRequestDocument::Cfd(document) => {
                let offset = byte_offset_from_position(document.source, position);
                cfd::completion_with_build(
                    document.source,
                    document.ast,
                    document.schema,
                    document.build,
                    offset,
                )
            }
            LspRequestDocument::Cft { build, document } => {
                completion_items(build, document, &position)
            }
            LspRequestDocument::Missing => Vec::new(),
        };
        result
    }
    pub(crate) fn hover(&self, uri: &str, position: LspPosition) -> Option<Hover> {
        let result = match self.request_document(uri) {
            LspRequestDocument::Cfd(document) => {
                let offset = byte_offset_from_position(document.source, position);
                cfd::hover(document.source, document.ast, document.schema, offset)
            }
            LspRequestDocument::Cft { build, document } => hover_at(build, document, &position),
            LspRequestDocument::Missing => None,
        };
        result
    }
    pub(crate) fn definition(&self, uri: &str, position: LspPosition) -> Vec<Location> {
        let result = match self.request_document(uri) {
            LspRequestDocument::Cfd(document) => {
                let offset = byte_offset_from_position(document.source, position);
                cfd_definition(&document, offset).into_iter().collect()
            }
            LspRequestDocument::Cft { build, document } => {
                definitions_at(build, document, &position)
            }
            LspRequestDocument::Missing => Vec::new(),
        };
        result
    }
    pub(crate) fn document_symbol(&self, uri: &str) -> Vec<DocumentSymbol> {
        let result = match self.request_document(uri) {
            LspRequestDocument::Cfd(document) => {
                cfd::document_symbols(document.source, document.ast)
            }
            LspRequestDocument::Cft { document, .. } => document_symbols(document),
            LspRequestDocument::Missing => Vec::new(),
        };
        result
    }
    pub(crate) fn formatting(&self, uri: &str) -> Vec<LanguageTextEdit> {
        let result = match self.request_document(uri) {
            LspRequestDocument::Cfd(document) => {
                if !document.syntax_valid {
                    return Vec::new();
                }
                let formatted = format_cfd(document.source);
                formatting_edits(document.source, &formatted)
            }
            LspRequestDocument::Cft { document, .. } => {
                if document.ast().is_none() {
                    return Vec::new();
                }
                let formatted = format_cft(&document.source);
                formatting_edits(&document.source, &formatted)
            }
            LspRequestDocument::Missing => Vec::new(),
        };
        result
    }
    pub(crate) fn semantic_tokens(&self, uri: &str) -> SemanticTokens {
        let result = match self.request_document(uri) {
            LspRequestDocument::Cfd(document) => {
                let mut result =
                    cfd::semantic_tokens(document.source, document.ast, document.schema);
                result.syntax_valid = document.syntax_valid;
                result
            }
            LspRequestDocument::Cft { build, document } => SemanticTokens {
                data: semantic_token_data(build, document),
                syntax_valid: document.ast.is_some(),
            },
            LspRequestDocument::Missing => SemanticTokens::default(),
        };
        result
    }
}
