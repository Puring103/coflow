mod compiler;
mod support;
mod type_checker;

use self::compiler::SchemaCompiler;
use crate::container::{CftContainer, ModuleId};
use crate::error::CftDiagnostics;
use crate::span::Span;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub struct CftSchemaModule {
    pub consts: Vec<CftSchemaConst>,
    pub types: Vec<CftSchemaType>,
    pub enums: Vec<CftSchemaEnum>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CftSchemaConst {
    pub module: ModuleId,
    pub name: String,
    pub value: CftConstValue,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CftConstValue {
    Int(i64),
    Float(f64),
    Bool(bool),
    String(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct CftSchemaType {
    pub module: ModuleId,
    pub name: String,
    pub parent: Option<String>,
    pub is_abstract: bool,
    pub is_sealed: bool,
    pub is_singleton: bool,
    pub fields: Vec<CftSchemaField>,     // 自身字段（不含继承）
    pub all_fields: Vec<CftSchemaField>, // 含继承的完整字段列表
    pub check: Option<CftSchemaCheckBlock>,
    pub annotations: Vec<CftAnnotation>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CftSchemaTypeRef {
    Int,
    Float,
    Bool,
    String,
    Named(String),
    Ref(String),
    Array(Box<CftSchemaTypeRef>),
    Dict(Box<CftSchemaTypeRef>, Box<CftSchemaTypeRef>),
    Nullable(Box<CftSchemaTypeRef>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dimension {
    Localized,
    Custom(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DimensionSpec {
    pub kind: Dimension,
    pub bucket: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CftSchemaField {
    pub name: String,
    pub ty: String,
    pub ty_ref: CftSchemaTypeRef,
    pub has_default: bool,
    pub default: Option<CftSchemaDefaultValue>,
    pub annotations: Vec<CftAnnotation>,
    pub dimension: Option<DimensionSpec>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CftSchemaDefaultValue {
    Null,
    Int(i64),
    Float(f64),
    Bool(bool),
    String(String),
    Enum {
        enum_name: String,
        variant: String,
        value: i64,
    },
    EmptyArray,
    EmptyObject,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CftSchemaCheckBlock {
    pub stmts: Vec<CftSchemaCheckStmt>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CftSchemaCheckStmt {
    Expr(CftSchemaCheckExpr),
    Quantifier {
        kind: CftSchemaQuantifierKind,
        binding: String,
        collection: CftSchemaCheckExpr,
        body: Vec<CftSchemaCheckStmt>,
        span: Span,
    },
    When {
        condition: CftSchemaCheckExpr,
        body: Vec<CftSchemaCheckStmt>,
        span: Span,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct CftSchemaCheckExpr {
    pub kind: CftSchemaCheckExprKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CftSchemaCheckExprKind {
    Int(i64),
    Float(f64),
    Bool(bool),
    Null,
    String(String),
    Name(String),
    Field {
        expr: Box<CftSchemaCheckExpr>,
        name: String,
    },
    Index {
        expr: Box<CftSchemaCheckExpr>,
        index: Box<CftSchemaCheckExpr>,
    },
    Is {
        expr: Box<CftSchemaCheckExpr>,
        predicate: CftSchemaTypePredicate,
    },
    Call {
        name: String,
        args: Vec<CftSchemaCheckExpr>,
    },
    MethodCall {
        receiver: Box<CftSchemaCheckExpr>,
        name: String,
        args: Vec<CftSchemaCheckExpr>,
    },
    BinOp {
        op: CftSchemaBinOp,
        lhs: Box<CftSchemaCheckExpr>,
        rhs: Box<CftSchemaCheckExpr>,
    },
    Unary {
        op: CftSchemaUnaryOp,
        expr: Box<CftSchemaCheckExpr>,
    },
    CmpChain {
        first: Box<CftSchemaCheckExpr>,
        rest: Vec<(CftSchemaCmpOp, CftSchemaCheckExpr)>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CftSchemaTypePredicate {
    Type(String),
    Null,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CftSchemaQuantifierKind {
    All,
    Any,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CftSchemaBinOp {
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
pub enum CftSchemaUnaryOp {
    Not,
    BitNot,
    Neg,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CftSchemaCmpOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CftSchemaEnum {
    pub module: ModuleId,
    pub name: String,
    pub variants: Vec<CftSchemaEnumVariant>,
    pub annotations: Vec<CftAnnotation>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CftSchemaEnumVariant {
    pub name: String,
    pub value: i64,
    pub annotations: Vec<CftAnnotation>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CftAnnotation {
    pub name: String,
    pub args: Vec<CftAnnotationValue>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CftAnnotationValue {
    Name(String),
    String(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    Null,
}

#[derive(Debug, Clone)]
pub(crate) struct CompiledSchema {
    pub(crate) modules: BTreeMap<ModuleId, CftSchemaModule>,
    pub(crate) consts: BTreeMap<String, CftSchemaConst>,
    pub(crate) types: BTreeMap<String, CftSchemaType>,
    pub(crate) enums: BTreeMap<String, CftSchemaEnum>,
}

pub(crate) fn compile_container(
    container: &CftContainer,
) -> Result<CompiledSchema, CftDiagnostics> {
    let mut compiler = SchemaCompiler::new(container);
    compiler.compile()
}
