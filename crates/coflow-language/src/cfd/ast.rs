use crate::Span;

#[derive(Debug, Clone, PartialEq)]
pub struct CfdAst {
    pub namespace: Option<CfdNamespaceDecl>,
    pub uses: Vec<CfdUseDecl>,
    pub records: Vec<CfdRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CfdNamespaceDecl {
    pub path: String,
    pub path_span: Span,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CfdUseDecl {
    pub path: String,
    pub path_span: Span,
    pub alias: Option<(String, Span)>,
    pub span: Span,
}

impl CfdUseDecl {
    #[must_use]
    pub fn local_name(&self) -> &str {
        self.alias.as_ref().map_or_else(
            || self.path.rsplit("::").next().unwrap_or(&self.path),
            |alias| alias.0.as_str(),
        )
    }
}

/// A top-level record or a record inside a group.
#[derive(Debug, Clone, PartialEq)]
pub struct CfdRecord {
    pub key: String,
    pub key_span: Span,
    /// Group declaration type for records nested in `Type { ... }`.
    pub group_type: Option<(String, Span)>,
    pub type_name: String,
    pub type_span: Span,
    pub fields: Vec<CfdField>,
    pub span: Span,
}

impl CfdRecord {
    pub fn fields(&self) -> impl Iterator<Item = &CfdField> {
        self.fields.iter()
    }
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
    BitExpr(CfdBitExpr),
    QuotedString(String, Span),
    FormattedString(CfdFormattedString),
    OptionNone(Span),
    OptionSome(Box<CfdValue>, Span),
    ResultOk(Box<CfdValue>, Span),
    ResultErr(Box<CfdValue>, Span),
    Function(CfdFunction),
    /// Object `{ ... }` or dict `{ ... }` — schema needed to distinguish.
    Block(CfdBlock),
    Array(Vec<CfdValue>, Span),
    Ref(CfdRef),
}

impl CfdValue {
    #[must_use]
    pub fn span(&self) -> Span {
        match self {
            Self::Scalar(_, s)
            | Self::QuotedString(_, s)
            | Self::OptionNone(s)
            | Self::OptionSome(_, s)
            | Self::ResultOk(_, s)
            | Self::ResultErr(_, s)
            | Self::Array(_, s) => *s,
            Self::Function(value) => value.span,
            Self::BitExpr(expr) => expr.span,
            Self::FormattedString(value) => value.span,
            Self::Block(b) => b.span,
            Self::Ref(r) => r.span,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CfdFunction {
    pub source: String,
    pub span: Span,
    pub body_span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CfdFormattedString {
    pub source: String,
    pub segments: Vec<CfdFormatSegment>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CfdFormatSegment {
    Text(String),
    Reference(CfdFieldReference),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CfdFieldReference {
    pub type_name: Option<String>,
    pub key: Option<String>,
    pub path: Vec<String>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CfdBitExpr {
    pub kind: CfdBitExprKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CfdBitExprKind {
    Value(String),
    Binary {
        op: CfdBitOp,
        lhs: Box<CfdBitExpr>,
        rhs: Box<CfdBitExpr>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CfdBitOp {
    Or,
    Xor,
    And,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CfdBlock {
    /// Optional type marker before `{`, e.g. `SubType { ... }`.
    pub type_marker: Option<(String, Span)>,
    pub fields: Vec<CfdField>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CfdRef {
    pub type_name: Option<(String, Span)>,
    pub key: (String, Span),
    pub span: Span,
}
