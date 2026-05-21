use std::rc::Rc;

use crate::ast::{AssignOp, BinaryOp, UnaryOp};
use crate::span::Span;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GlobalId(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FunctionId(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ClassId(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EnumId(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct VariantId(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LocalId(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct UpvalueId(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BuiltinId(pub usize);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ty {
    Int,
    Float,
    Bool,
    String,
    Null,
    Any,
    Array(Box<Ty>),
    Dict(Box<Ty>, Box<Ty>),
    Class(ClassId),
    Enum(EnumId),
    Function,
    FunctionSig { params: Vec<Ty>, return_ty: Box<Ty> },
    Iterator,
    Error,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(Rc<str>),
    Array(Rc<Vec<Value>>),
    Dict(Rc<Vec<(Value, Value)>>),
    Object {
        class: Option<ClassId>,
        fields: Rc<Vec<(String, Value)>>,
    },
    Range {
        start: i64,
        end: i64,
        inclusive: bool,
    },
    EnumVariant(EnumId, VariantId),
    Closure(Rc<ClosureData>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClosureData {
    pub fn_id: FunctionId,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HirModule {
    pub globals: Vec<HirGlobal>,
    pub functions: Vec<HirFunction>,
    pub classes: Vec<HirClass>,
    pub enums: Vec<HirEnum>,
    pub config_eval_order: Vec<GlobalId>,
    pub config_values: Vec<(GlobalId, Value)>,
}

impl HirModule {
    pub fn new() -> Self {
        Self {
            globals: Vec::new(),
            functions: Vec::new(),
            classes: Vec::new(),
            enums: Vec::new(),
            config_eval_order: Vec::new(),
            config_values: Vec::new(),
        }
    }

    pub fn global(&self, id: GlobalId) -> Option<&HirGlobal> {
        self.globals.get(id.0)
    }

    pub fn global_mut(&mut self, id: GlobalId) -> Option<&mut HirGlobal> {
        self.globals.get_mut(id.0)
    }

    pub fn function(&self, id: FunctionId) -> Option<&HirFunction> {
        self.functions.get(id.0)
    }

    pub fn function_mut(&mut self, id: FunctionId) -> Option<&mut HirFunction> {
        self.functions.get_mut(id.0)
    }

    pub fn class(&self, id: ClassId) -> Option<&HirClass> {
        self.classes.get(id.0)
    }

    pub fn enum_(&self, id: EnumId) -> Option<&HirEnum> {
        self.enums.get(id.0)
    }

    pub fn config_value(&self, id: GlobalId) -> Option<&Value> {
        self.config_values
            .iter()
            .find_map(|(value_id, value)| (*value_id == id).then_some(value))
    }
}

impl Default for HirModule {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum HirGlobal {
    Config {
        id: GlobalId,
        name: String,
        ty: Option<Ty>,
        value: HirExpr,
        span: Span,
    },
    Var {
        id: GlobalId,
        name: String,
        local: bool,
        ty: Option<Ty>,
        init: Option<HirExpr>,
        span: Span,
    },
    Function {
        id: GlobalId,
        name: String,
        local: bool,
        fn_id: FunctionId,
        span: Span,
    },
    Class {
        id: GlobalId,
        class_id: ClassId,
        span: Span,
    },
    Enum {
        id: GlobalId,
        enum_id: EnumId,
        span: Span,
    },
    Import {
        id: GlobalId,
        name: String,
        span: Span,
    },
    Builtin {
        id: GlobalId,
        builtin_id: BuiltinId,
        name: String,
        span: Span,
    },
}

impl HirGlobal {
    pub fn id(&self) -> GlobalId {
        match self {
            HirGlobal::Config { id, .. }
            | HirGlobal::Var { id, .. }
            | HirGlobal::Function { id, .. }
            | HirGlobal::Class { id, .. }
            | HirGlobal::Enum { id, .. }
            | HirGlobal::Import { id, .. }
            | HirGlobal::Builtin { id, .. } => *id,
        }
    }

    pub fn name(&self) -> Option<&str> {
        match self {
            HirGlobal::Config { name, .. }
            | HirGlobal::Var { name, .. }
            | HirGlobal::Function { name, .. }
            | HirGlobal::Import { name, .. }
            | HirGlobal::Builtin { name, .. } => Some(name),
            HirGlobal::Class { .. } | HirGlobal::Enum { .. } => None,
        }
    }

    pub fn span(&self) -> Span {
        match self {
            HirGlobal::Config { span, .. }
            | HirGlobal::Var { span, .. }
            | HirGlobal::Function { span, .. }
            | HirGlobal::Class { span, .. }
            | HirGlobal::Enum { span, .. }
            | HirGlobal::Import { span, .. }
            | HirGlobal::Builtin { span, .. } => *span,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct HirFunction {
    pub is_iter: bool,
    pub params: Vec<HirParam>,
    pub return_ty: Option<Ty>,
    pub locals: Vec<HirLocal>,
    pub upvalues: Vec<UpvalueDesc>,
    pub body: Vec<HirStmt>,
    pub span: Span,
    pub signature: Ty,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HirParam {
    pub local_id: LocalId,
    pub name: String,
    pub ty: Option<Ty>,
    pub default: Option<HirExpr>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HirLocal {
    pub name: String,
    pub ty: Option<Ty>,
    pub is_captured: bool,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UpvalueDesc {
    Local(LocalId),
    Upvalue(UpvalueId),
}

#[derive(Debug, Clone, PartialEq)]
pub struct HirClass {
    pub name: String,
    pub local: bool,
    pub fields: Vec<HirClassField>,
    pub checks: Vec<HirCheckArm>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HirClassField {
    pub name: String,
    pub ty: Ty,
    pub default: Option<HirExpr>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HirCheckArm {
    pub cond: HirExpr,
    pub message: HirExpr,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HirEnum {
    pub name: String,
    pub local: bool,
    pub variants: Vec<HirEnumVariant>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HirEnumVariant {
    pub name: String,
    pub value: i64,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HirStmt {
    Local {
        id: LocalId,
        init: Option<HirExpr>,
        span: Span,
    },
    Assign {
        target: HirAssignTarget,
        op: AssignOp,
        value: HirExpr,
        span: Span,
    },
    Expr(HirExpr),
    If {
        cond: HirExpr,
        then_: Vec<HirStmt>,
        else_: Option<Vec<HirStmt>>,
        span: Span,
    },
    While {
        cond: HirExpr,
        body: Vec<HirStmt>,
        span: Span,
    },
    Loop {
        body: Vec<HirStmt>,
        span: Span,
    },
    ForIn {
        item: LocalId,
        iter: HirExpr,
        body: Vec<HirStmt>,
        span: Span,
    },
    ForRange {
        var: LocalId,
        start: HirExpr,
        end: HirExpr,
        inclusive: bool,
        body: Vec<HirStmt>,
        span: Span,
    },
    Break(Span),
    Continue(Span),
    Return {
        value: Option<HirExpr>,
        span: Span,
    },
    Throw {
        value: HirExpr,
        span: Span,
    },
    TryCatch {
        try_: Vec<HirStmt>,
        err: LocalId,
        catch_: Vec<HirStmt>,
        span: Span,
    },
    Yield {
        value: HirExpr,
        span: Span,
    },
    YieldFrom {
        value: HirExpr,
        span: Span,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum HirAssignTarget {
    Local(LocalId),
    Upvalue(UpvalueId),
    Global(GlobalId),
    Field {
        obj: Box<HirExpr>,
        field: String,
        span: Span,
    },
    Index {
        obj: Box<HirExpr>,
        index: Box<HirExpr>,
        span: Span,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum HirExpr {
    Const {
        value: Value,
        span: Span,
    },
    Local {
        id: LocalId,
        span: Span,
    },
    Upvalue {
        id: UpvalueId,
        span: Span,
    },
    Global {
        id: GlobalId,
        span: Span,
    },
    Variant {
        enum_id: EnumId,
        variant: VariantId,
        span: Span,
    },
    Closure {
        fn_id: FunctionId,
        span: Span,
    },
    SelfField {
        name: String,
        span: Span,
    },
    Unary {
        op: UnaryOp,
        expr: Box<HirExpr>,
        span: Span,
    },
    Binary {
        op: BinaryOp,
        lhs: Box<HirExpr>,
        rhs: Box<HirExpr>,
        span: Span,
    },
    AndChain {
        exprs: Vec<HirExpr>,
        span: Span,
    },
    NullCoalesce {
        left: Box<HirExpr>,
        right: Box<HirExpr>,
        span: Span,
    },
    Call {
        callee: Box<HirExpr>,
        args: Vec<HirArg>,
        span: Span,
    },
    Field {
        obj: Box<HirExpr>,
        field: String,
        span: Span,
    },
    OptField {
        obj: Box<HirExpr>,
        field: String,
        span: Span,
    },
    Index {
        obj: Box<HirExpr>,
        index: Box<HirExpr>,
        span: Span,
    },
    OptIndex {
        obj: Box<HirExpr>,
        index: Box<HirExpr>,
        span: Span,
    },
    Array {
        elements: Vec<HirExpr>,
        span: Span,
    },
    Object {
        class: Option<ClassId>,
        fields: Vec<(String, HirExpr)>,
        spreads: Vec<HirExpr>,
        span: Span,
    },
    Dict {
        entries: Vec<(HirExpr, HirExpr)>,
        span: Span,
    },
    Range {
        start: Box<HirExpr>,
        end: Box<HirExpr>,
        inclusive: bool,
        span: Span,
    },
    If {
        cond: Box<HirExpr>,
        then_expr: Box<HirExpr>,
        else_expr: Box<HirExpr>,
        span: Span,
    },
    TypeGuard {
        expr: Box<HirExpr>,
        ty: Ty,
        span: Span,
    },
    Error(Span),
}

impl HirExpr {
    pub fn span(&self) -> Span {
        match self {
            HirExpr::Const { span, .. }
            | HirExpr::Local { span, .. }
            | HirExpr::Upvalue { span, .. }
            | HirExpr::Global { span, .. }
            | HirExpr::Variant { span, .. }
            | HirExpr::Closure { span, .. }
            | HirExpr::SelfField { span, .. }
            | HirExpr::Unary { span, .. }
            | HirExpr::Binary { span, .. }
            | HirExpr::AndChain { span, .. }
            | HirExpr::NullCoalesce { span, .. }
            | HirExpr::Call { span, .. }
            | HirExpr::Field { span, .. }
            | HirExpr::OptField { span, .. }
            | HirExpr::Index { span, .. }
            | HirExpr::OptIndex { span, .. }
            | HirExpr::Array { span, .. }
            | HirExpr::Object { span, .. }
            | HirExpr::Dict { span, .. }
            | HirExpr::Range { span, .. }
            | HirExpr::If { span, .. }
            | HirExpr::TypeGuard { span, .. }
            | HirExpr::Error(span) => *span,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct HirArg {
    pub name: Option<String>,
    pub value: HirExpr,
    pub span: Span,
}
