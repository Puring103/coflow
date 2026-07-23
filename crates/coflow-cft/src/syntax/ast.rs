// AST nodes deliberately keep span fields (`Bool(_, Span)`, `SignedInt.span`,
// `AnnotationArg::span()`, `CheckStmt::span()`) even when current passes do not
// consume them. They are part of the canonical AST shape and are exercised by
// downstream tooling (IDE diagnostics, codegen). Suppress the resulting
// `dead_code` warnings here rather than in individual definitions.
use crate::syntax::Span;

#[derive(Debug, Clone)]
pub struct ModuleAst {
    pub items: Vec<Item>,
    pub dangling_annotations: Vec<Annotation>,
}

#[derive(Debug, Clone)]
pub enum Item {
    Const(ConstDef),
    Enum(EnumDef),
    Type(TypeDef),
    Check(TopLevelCheckDef),
}

impl Item {
    #[must_use]
    pub const fn span(&self) -> Span {
        match self {
            Self::Const(definition) => definition.span,
            Self::Enum(definition) => definition.span,
            Self::Type(definition) => definition.span,
            Self::Check(definition) => definition.span,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TopLevelCheckDef {
    pub name: String,
    pub name_span: Span,
    pub block: CheckBlock,
    pub annotations: Vec<Annotation>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct ConstDef {
    pub name: String,
    pub name_span: Span,
    pub ty: Option<TypeRef>,
    pub value: ConstLiteral,
    pub annotations: Vec<Annotation>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct EnumDef {
    pub name: String,
    pub name_span: Span,
    pub variants: Vec<EnumVariant>,
    pub annotations: Vec<Annotation>,
    pub dangling_annotations: Vec<Annotation>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct EnumVariant {
    pub name: String,
    pub name_span: Span,
    pub value: Option<SignedInt>,
    pub annotations: Vec<Annotation>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct TypeDef {
    pub name: String,
    pub name_span: Span,
    pub is_abstract: bool,
    pub abstract_span: Option<Span>,
    pub is_sealed: bool,
    pub sealed_span: Option<Span>,
    pub parent: Option<NameRef>,
    pub fields: Vec<FieldDef>,
    pub check: Option<CheckBlock>,
    pub annotations: Vec<Annotation>,
    pub dangling_annotations: Vec<Annotation>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct FieldDef {
    pub name: String,
    pub name_span: Span,
    pub ty: TypeRef,
    pub default: Option<DefaultExpr>,
    pub annotations: Vec<Annotation>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct NameRef {
    pub name: String,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Annotation {
    pub name: String,
    pub name_span: Span,
    pub args: Vec<AnnotationArg>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum AnnotationArg {
    Name(NameRef),
    String(String, Span),
    Int(i64, Span),
    Float(f64, Span),
    Bool(bool, Span),
    Null(Span),
}

impl AnnotationArg {
    #[must_use]
    pub fn span(&self) -> Span {
        match self {
            Self::Name(name) => name.span,
            Self::String(_, span)
            | Self::Int(_, span)
            | Self::Float(_, span)
            | Self::Bool(_, span)
            | Self::Null(span) => *span,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SignedInt {
    pub value: i64,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum ConstLiteral {
    Int(i64, Span),
    Float(f64, Span),
    Bool(bool, Span),
    String(String, Span),
}

impl ConstLiteral {
    #[must_use]
    pub fn span(&self) -> Span {
        match self {
            Self::Int(_, span)
            | Self::Float(_, span)
            | Self::Bool(_, span)
            | Self::String(_, span) => *span,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeRefKind {
    Int,
    Float,
    Bool,
    String,
    Named(String),
    Ref(Box<TypeRef>),
    Array(Box<TypeRef>),
    Dict(Box<TypeRef>, Box<TypeRef>),
    Nullable(Box<TypeRef>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeRef {
    pub kind: TypeRefKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct DefaultExpr {
    pub kind: DefaultExprKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum DefaultExprKind {
    Int(i64),
    Float(f64),
    Bool(bool),
    Null,
    String(String),
    Name(NameRef),
    EnumVariant {
        enum_name: NameRef,
        variant: NameRef,
    },
    Array(Vec<DefaultExpr>),
    Object(Vec<(NameRef, DefaultExpr)>),
}

#[derive(Debug, Clone)]
pub struct CheckBlock {
    pub stmts: Vec<CheckStmt>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct CheckMessage {
    pub kind: CheckMessageKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum CheckMessageKind {
    String(String),
    Formatted(Vec<CheckFormatSegment>),
}

#[derive(Debug, Clone)]
pub enum CheckFormatSegment {
    Text(String, Span),
    Expr(CheckExpr),
}

#[derive(Debug, Clone)]
pub enum CheckStmt {
    Expr {
        condition: CheckExpr,
        message: Option<CheckMessage>,
        span: Span,
    },
    Quantifier {
        kind: QuantifierKind,
        bindings: Vec<NameRef>,
        collection: CheckExpr,
        body: Vec<CheckStmt>,
        span: Span,
    },
    When {
        condition: CheckExpr,
        body: Vec<CheckStmt>,
        span: Span,
    },
}

impl CheckStmt {
    #[must_use]
    pub fn span(&self) -> Span {
        match self {
            Self::Expr { span, .. } | Self::Quantifier { span, .. } | Self::When { span, .. } => {
                *span
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuantifierKind {
    All,
    Any,
    None,
}

#[derive(Debug, Clone)]
pub struct CheckExpr {
    pub kind: CheckExprKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum CheckExprKind {
    Int(i64),
    Float(f64),
    Bool(bool),
    Null,
    String(String),
    FormattedString(Vec<CheckFormatSegment>),
    Name(String),
    Field {
        expr: Box<CheckExpr>,
        name: NameRef,
    },
    SafeField {
        expr: Box<CheckExpr>,
        name: NameRef,
    },
    Index {
        expr: Box<CheckExpr>,
        index: Box<CheckExpr>,
    },
    SafeIndex {
        expr: Box<CheckExpr>,
        index: Box<CheckExpr>,
    },
    Coalesce {
        lhs: Box<CheckExpr>,
        rhs: Box<CheckExpr>,
    },
    Is {
        expr: Box<CheckExpr>,
        predicate: TypePredicate,
    },
    Call {
        name: NameRef,
        args: Vec<CheckExpr>,
    },
    MethodCall {
        receiver: Box<CheckExpr>,
        name: NameRef,
        args: Vec<CheckExpr>,
    },
    BinOp {
        op: BinOp,
        lhs: Box<CheckExpr>,
        rhs: Box<CheckExpr>,
    },
    Unary {
        op: UnaryOp,
        expr: Box<CheckExpr>,
    },
    CmpChain {
        first: Box<CheckExpr>,
        rest: Vec<(CmpOp, CheckExpr)>,
    },
}

#[derive(Debug, Clone)]
pub enum TypePredicate {
    Type(NameRef),
    Null(Span),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Or,
    And,
    BitOr,
    BitXor,
    BitAnd,
    Add,
    Sub,
    Shl,
    Shr,
    Mul,
    Div,
    IntDiv,
    Mod,
    Pow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Not,
    BitNot,
    Neg,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmpOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}
