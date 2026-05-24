use crate::span::Span;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub message: String,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseErrors {
    pub errors: Vec<ParseError>,
}

impl ParseErrors {
    pub fn one(message: impl Into<String>, span: Span) -> Self {
        Self {
            errors: vec![ParseError {
                message: message.into(),
                span,
            }],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildError {
    pub message: String,
    pub span: Option<Span>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildErrors {
    pub errors: Vec<BuildError>,
}

impl BuildErrors {
    #[must_use]
    pub fn new(errors: Vec<BuildError>) -> Self {
        Self { errors }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckError {
    pub message: String,
    pub span: Option<Span>,
}

#[derive(Debug)]
pub enum CfcError {
    Parse(ParseErrors),
    Module(crate::container::ModuleError),
    Import(crate::container::BindImportError),
    Resolve(crate::container::ResolveError),
    Build(BuildErrors),
}
