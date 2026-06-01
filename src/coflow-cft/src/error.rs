use crate::span::Span;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub message: String,
    pub span: Span,
    pub kind: ParseErrorKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseErrorKind {
    Lex,
    Syntax,
    Module,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseErrors {
    pub errors: Vec<ParseError>,
}

impl ParseErrors {
    pub fn one(message: impl Into<String>, span: Span) -> Self {
        Self::one_kind(ParseErrorKind::Syntax, message, span)
    }

    pub fn one_kind(kind: ParseErrorKind, message: impl Into<String>, span: Span) -> Self {
        Self {
            errors: vec![ParseError {
                message: message.into(),
                span,
                kind,
            }],
        }
    }
}
