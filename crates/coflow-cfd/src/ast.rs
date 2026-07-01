use coflow_cft::Span;

#[derive(Debug, Clone, PartialEq)]
pub struct CfdAst {
    pub records: Vec<CfdRecord>,
}

/// A top-level record or a record inside a group.
#[derive(Debug, Clone, PartialEq)]
pub struct CfdRecord {
    pub key: String,
    pub key_span: Span,
    pub type_name: String,
    pub type_span: Span,
    pub entries: Vec<CfdBlockEntry>,
    pub fields: Vec<CfdField>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CfdField {
    pub name: String,
    pub name_span: Span,
    pub value: CfdValue,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CfdValue {
    /// Unquoted token — could be int, float, bool, enum variant, etc.
    Scalar(String, Span),
    QuotedString(String, Span),
    Null(Span),
    /// Object `{ ... }` or dict `{ ... }` — schema needed to distinguish.
    Block(CfdBlock),
    Array(Vec<CfdValue>, Span),
    Ref(CfdRef),
    Spread(Box<CfdValue>, Span),
}

impl CfdValue {
    #[must_use]
    pub fn span(&self) -> Span {
        match self {
            Self::Scalar(_, s)
            | Self::QuotedString(_, s)
            | Self::Null(s)
            | Self::Array(_, s)
            | Self::Spread(_, s) => *s,
            Self::Block(b) => b.span,
            Self::Ref(r) => r.span,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CfdBlock {
    /// Optional type marker before `{`, e.g. `SubType { ... }`.
    pub type_marker: Option<(String, Span)>,
    pub entries: Vec<CfdBlockEntry>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CfdBlockEntry {
    Field(CfdField),
    Spread(CfdValue, Span),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CfdRef {
    pub key: (String, Span),
    pub span: Span,
}
