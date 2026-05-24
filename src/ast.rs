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

#[derive(Debug, Clone)]
pub struct CheckBlock {
    pub span: Span,
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
    Any,
    Array(Box<TypeRef>),
    Dict(Box<TypeRef>, Box<TypeRef>),
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
