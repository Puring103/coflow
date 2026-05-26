use crate::container::ImportId;
use crate::span::Span;

#[derive(Debug, Clone)]
pub struct ModuleAst {
    pub imports: Vec<UseDecl>,
    pub items: Vec<Item>,
}

#[derive(Debug, Clone)]
pub struct UseDecl {
    pub id: ImportId,
    pub path: String,
    pub alias: String,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum Item {
    Type(TypeDef),
    Enum(EnumDef),
    Data(DataDef),
    Check(CheckBlock),
}

#[derive(Debug, Clone)]
pub struct TypeDef {
    pub name: String,
    pub fields: Vec<FieldDef>,
    #[allow(dead_code)]
    pub check: Option<CheckBlock>,
    pub alias: Option<TypeRef>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct FieldDef {
    pub name: String,
    pub ty: TypeRef,
    pub default: Option<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct EnumDef {
    pub name: String,
    pub variants: Vec<EnumVariant>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct EnumVariant {
    pub name: String,
    pub value: Option<i64>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct DataDef {
    pub name: String,
    pub ty: Option<TypeRef>,
    pub value: Expr,
    pub span: Span,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct CheckBlock {
    pub stmts: Vec<CondStmt>,
    pub span: Span,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum CondStmt {
    Expr(CheckExpr),
    Quantifier {
        kind: QuantifierKind,
        binding: String,
        collection: CheckExpr,
        body: Vec<CondStmt>,
        span: Span,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuantifierKind {
    All,
    Any,
    None,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct CheckExpr {
    pub kind: CheckExprKind,
    pub span: Span,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum CheckExprKind {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    Name(String),
    Field {
        expr: Box<CheckExpr>,
        name: String,
    },
    Index {
        expr: Box<CheckExpr>,
        index: Box<CheckExpr>,
    },
    Is {
        expr: Box<CheckExpr>,
        ty: TypeName,
    },
    Call {
        name: String,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Or,
    And,
    BitOr,
    BitXor,
    BitAnd,
    Add,
    Sub,
    Mul,
    Div,
    IntDiv,
    Mod,
    Pow,
    Shl,
    Shr,
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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TypeName {
    Local(String),
    Imported { alias: String, name: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TypeRef {
    Int,
    Float,
    Bool,
    String,
    StringLiteral(String),
    Any,
    Array(Box<TypeRef>),
    Dict(Box<TypeRef>, Box<TypeRef>),
    Union(Vec<TypeRef>),
    Named(TypeName),
}

#[derive(Debug, Clone)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum ExprKind {
    Int(i64),
    Float(f64),
    Bool(bool),
    String(String),
    Name(String),
    Qualified(Vec<String>),
    Path {
        root: String,
        segments: Vec<PathSegment>,
    },
    TypedObject {
        ty: TypeName,
        fields: Vec<ObjectField>,
    },
    Object(Vec<ObjectField>),
    Array(Vec<Expr>),
    Dict(Vec<(Expr, Expr)>),
}

#[derive(Debug, Clone)]
pub enum PathSegment {
    Field(String),
    Index(usize),
}

#[derive(Debug, Clone)]
pub struct ObjectField {
    pub name: String,
    pub value: Expr,
    pub span: Span,
}
