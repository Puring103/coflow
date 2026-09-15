// AST nodes deliberately keep span fields (`Bool(_, Span)`, `SignedInt.span`,
// `AnnotationArg::span()`) even when current passes do not
// consume them. They are part of the canonical AST shape and are exercised by
// downstream tooling (IDE diagnostics, codegen). Suppress the resulting
// `dead_code` warnings here rather than in individual definitions.
use crate::source::Span;

#[derive(Debug, Clone)]
pub struct ModuleAst {
    pub namespace: Option<NamePath>,
    pub imports: Vec<Import>,
    pub items: Vec<Item>,
    pub dangling_annotations: Vec<Annotation>,
}

#[derive(Debug, Clone)]
pub struct Import {
    pub path: NamePath,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TypeKind {
    Table,
    Singleton,
    Data,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamePath {
    pub segments: Vec<NameRef>,
    pub span: Span,
}

impl NamePath {
    #[must_use]
    pub fn canonical(&self) -> String {
        self.segments
            .iter()
            .map(|segment| segment.name.as_str())
            .collect::<Vec<_>>()
            .join("::")
    }
}

#[derive(Debug, Clone)]
pub enum Item {
    Const(ConstDef),
    Enum(EnumDef),
    Type(TypeDef),
    TypeAlias(TypeAliasDef),
    Check(TopLevelCheckDef),
}

impl Item {
    #[must_use]
    pub const fn span(&self) -> Span {
        match self {
            Self::Const(definition) => definition.span,
            Self::Enum(definition) => definition.span,
            Self::Type(definition) => definition.span,
            Self::TypeAlias(definition) => definition.span,
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
    pub value: DefaultExpr,
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
    pub kind: TypeKind,
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
pub struct TypeAliasDef {
    pub name: String,
    pub name_span: Span,
    pub target: TypeRef,
    pub annotations: Vec<Annotation>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
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
}

impl AnnotationArg {
    #[must_use]
    pub fn span(&self) -> Span {
        match self {
            Self::Name(name) => name.span,
            Self::String(_, span)
            | Self::Int(_, span)
            | Self::Float(_, span)
            | Self::Bool(_, span) => *span,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SignedInt {
    pub value: i64,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeRefKind {
    Int,
    Float,
    Bool,
    String,
    FString,
    Named(String),
    Array(Box<TypeRef>),
    Dict(Box<TypeRef>, Box<TypeRef>),
    Option(Box<TypeRef>),
    Function(Vec<FunctionParameterRef>, Box<TypeRef>),
    Unit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionParameterRef {
    pub name: Option<NameRef>,
    pub value_type: TypeRef,
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
    OptionNone,
    OptionSome(Box<DefaultExpr>),
    String(String),
    FormattedString(String),
    Function {
        signature: TypeRef,
        source: String,
    },
    BitExpr {
        op: DefaultBitOp,
        lhs: Box<DefaultExpr>,
        rhs: Box<DefaultExpr>,
    },
    StaticPath(NamePath),
    RecordReference(NamePath),
    Array(Vec<DefaultExpr>),
    Object(Vec<(NameRef, DefaultExpr)>),
    TypedObject {
        type_name: NamePath,
        fields: Vec<(NameRef, DefaultExpr)>,
    },
    Dictionary(Vec<(DefaultExpr, DefaultExpr)>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefaultBitOp {
    Or,
    Xor,
    And,
}

#[derive(Debug, Clone)]
pub struct CheckBlock {
    /// 保留程序源码；函数编译阶段尚未实现时不生成假检查语句。
    pub source: String,
    pub span: Span,
}
