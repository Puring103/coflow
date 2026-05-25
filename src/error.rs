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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildError {
    pub message: String,
    pub span: Option<Span>,
    pub kind: BuildErrorKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildErrorKind {
    Module,
    Import,
    DuplicateName,
    DuplicateField,
    DuplicateEnumVariant,
    DuplicateEnumValue,
    UnknownName,
    UnknownType,
    UnknownEnumVariant,
    TypeMismatch,
    MissingRequiredField,
    ExtraField,
    InvalidDefault,
    InvalidDictKeyType,
    Inference,
    Cycle,
    Path,
    Other,
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

impl BuildError {
    #[must_use]
    pub fn new(kind: BuildErrorKind, message: impl Into<String>, span: Option<Span>) -> Self {
        Self {
            message: message.into(),
            span,
            kind,
        }
    }

    #[must_use]
    pub fn other(message: impl Into<String>, span: Option<Span>) -> Self {
        Self::new(BuildErrorKind::Other, message, span)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckError {
    pub message: String,
    pub span: Option<Span>,
    pub kind: CheckErrorKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckErrorKind {
    CondFailed {
        source: String,
        evaluated: String,
        context: String,
    },
    AllFailed {
        source: String,
        context: String,
        total: usize,
        failed: Vec<AllFailedItem>,
    },
    EvalError {
        message: String,
        context: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AllFailedItem {
    pub key: String,
    pub errors: Vec<CheckError>,
}

#[derive(Debug)]
pub enum CfcError {
    Parse(ParseErrors),
    Module(crate::container::ModuleError),
    Import(crate::container::BindImportError),
    Resolve(crate::container::ResolveError),
    Build(BuildErrors),
}
