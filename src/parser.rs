use crate::ast::Module;
use crate::lexer::{lex, LexErrorKind};
use crate::span::Span;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseOutput {
    pub module: Option<Module>,
    pub errors: Vec<ParseError>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub kind: ParseErrorKind,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseErrorKind {
    Lex(LexErrorKind),
    UnexpectedEof,
    UnexpectedToken,
    ExpectedItem,
    ExpectedType,
    ExpectedExpression,
    ExpectedIdentifier,
    ExpectedToken,
    InvalidAssignmentTarget,
    MissingCatch,
    UnsupportedParserNotImplemented,
}

pub fn parse_module(source: &str) -> ParseOutput {
    let lexed = lex(source);
    if !lexed.errors.is_empty() {
        return ParseOutput {
            module: None,
            errors: lexed
                .errors
                .into_iter()
                .map(|error| ParseError {
                    kind: ParseErrorKind::Lex(error.kind),
                    span: error.span,
                })
                .collect(),
        };
    }

    ParseOutput {
        module: None,
        errors: vec![ParseError {
            kind: ParseErrorKind::UnsupportedParserNotImplemented,
            span: Span {
                start: 0,
                end: source.len(),
            },
        }],
    }
}
