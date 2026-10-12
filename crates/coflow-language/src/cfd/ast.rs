use crate::Span;

#[derive(Debug, Clone, PartialEq)]
pub struct CfdAst {
    pub records: Vec<CfdRecord>,
    pub dimensions: Vec<CfdDimensionBlock>,
}

/// 独立维度覆盖块；语法层只记录目标，不创建基础记录。
#[derive(Debug, Clone, PartialEq)]
pub struct CfdDimensionBlock {
    pub dimension: String,
    pub dimension_span: Span,
    pub variant: String,
    pub variant_span: Span,
    pub records: Vec<CfdRecord>,
    pub span: Span,
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
            | Self::Array(_, s) => *s,
            Self::BitExpr(expr) => expr.span,
            Self::FormattedString(value) => value.span,
            Self::Block(b) => b.span,
            Self::Ref(r) => r.span,
        }
    }
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
    pub(crate) depth: u64,
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
