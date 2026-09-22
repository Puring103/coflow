//! CFT、CFD 与模板共用的函数语法；这里不解析声明身份或执行表达式。
mod parser;
use crate::{source::Span, syntax::ast::TypeRef};
pub use parser::{parse_checks, parse_expression, parse_function, parse_function_with_limits};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntaxError {
    pub span: Span,
    pub message: String,
}
impl std::fmt::Display for SyntaxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}..{}: {}",
            self.span.start, self.span.end, self.message
        )
    }
}
impl std::error::Error for SyntaxError {}

#[derive(Debug, Clone, PartialEq)]
pub struct Function {
    pub parameters: Vec<(String, TypeRef)>,
    pub result: TypeRef,
    pub body: Block,
    pub span: Span,
}
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub statements: Vec<Statement>,
    pub tail: Option<Box<Expr>>,
    pub span: Span,
}
#[derive(Debug, Clone, PartialEq)]
pub struct Statement {
    pub kind: StatementKind,
    pub span: Span,
}
#[derive(Debug, Clone, PartialEq)]
pub enum StatementKind {
    Variable {
        name: String,
        ty: TypeRef,
        value: Expr,
    },
    Assign {
        name: String,
        operator: String,
        value: Expr,
    },
    Set { target: Expr, value: Expr },
    Expression(Expr),
    While {
        condition: Expr,
        body: Block,
    },
    For {
        bindings: Vec<String>,
        iterable: Expr,
        body: Block,
    },
    Break,
    Continue,
}
#[derive(Debug, Clone, PartialEq)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}
#[derive(Debug, Clone, PartialEq)]
pub enum ExprKind {
    Unit,
    None,
    Bool(bool),
    /// 保留原始数字；i32 最小值由带符号表达式的类型检查整体处理。
    Number(String),
    String(String),
    Template(Vec<TemplatePart>),
    Name(String),
    Reference {
        type_name: Option<String>,
        key: String,
    },
    Array(Vec<Expr>),
    Dictionary(Vec<(Expr, Expr)>),
    Object {
        type_name: String,
        fields: Vec<(String, Expr)>,
    },
    Build { source: BuildSource, binding: String, body: Block },
    Unary {
        operator: String,
        value: Box<Expr>,
    },
    Binary {
        operator: String,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Field {
        value: Box<Expr>,
        name: String,
    },
    Index {
        value: Box<Expr>,
        index: Box<Expr>,
    },
    Call {
        function: Box<Expr>,
        arguments: Vec<Expr>,
    },
    IsType {
        value: Box<Expr>,
        name: String,
    },
    IsSome {
        value: Box<Expr>,
        binding: String,
    },
    Propagate(Box<Expr>),
    If {
        condition: Box<Expr>,
        then: Block,
        otherwise: Option<Box<Expr>>,
    },
    Block(Block),
    Function(Function),
    Return(Option<Box<Expr>>),
}
#[derive(Debug, Clone, PartialEq)]
pub enum TemplatePart {
    Text(String),
    Expression(Expr),
}
#[derive(Debug, Clone, PartialEq)]
pub enum BuildSource {
    Type(TypeRef),
    Value(Box<Expr>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Check {
    pub name: Option<String>,
    pub body: Block,
    pub span: Span,
}
