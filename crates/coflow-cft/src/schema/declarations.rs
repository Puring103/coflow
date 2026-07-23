use super::CftValueType;
use crate::module::ModuleId;
use crate::syntax::Span;
use crate::{
    BucketName, CheckName, ConstName, DimensionName, EnumName, EnumVariantName, FieldName, TypeName,
};
use std::collections::BTreeMap;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq)]
pub struct CftConst {
    pub module: ModuleId,
    pub name: ConstName,
    pub value: CftConstValue,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CftTopLevelCheck {
    pub module: ModuleId,
    pub name: CheckName,
    pub block: CftSchemaCheckBlock,
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
#[allow(clippy::struct_excessive_bools)] // CFT modifiers and annotation semantics are orthogonal.
pub struct CftType {
    pub module: ModuleId,
    pub name: TypeName,
    pub parent: Option<TypeName>,
    pub is_abstract: bool,
    pub is_sealed: bool,
    pub is_struct: bool,
    pub is_singleton: bool,
    pub id_as_enum: Option<EnumName>,
    pub(crate) own_fields: Vec<Arc<CftField>>,
    pub(crate) all_fields: Vec<Arc<CftField>>,
    pub(crate) field_by_name: BTreeMap<FieldName, usize>,
    pub check: Option<CftSchemaCheckBlock>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CftFieldDimension {
    pub dimension: DimensionName,
    pub bucket: Option<BucketName>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CftField {
    pub declaring_type: TypeName,
    pub name: FieldName,
    pub value_type: CftValueType,
    pub default: Option<CftSchemaDefaultValue>,
    pub is_expand: bool,
    pub dimension: Option<CftFieldDimension>,
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
        enum_name: EnumName,
        variant: EnumVariantName,
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
pub struct CftSchemaCheckMessage {
    pub kind: CftSchemaCheckMessageKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CftSchemaCheckMessageKind {
    String(String),
    Formatted(Vec<CftSchemaCheckFormatSegment>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum CftSchemaCheckFormatSegment {
    Text(String, Span),
    Expr(CftSchemaCheckExpr),
}

#[derive(Debug, Clone, PartialEq)]
pub enum CftSchemaCheckStmt {
    Expr {
        condition: CftSchemaCheckExpr,
        message: Option<CftSchemaCheckMessage>,
        span: Span,
    },
    Quantifier {
        kind: CftSchemaQuantifierKind,
        bindings: CftSchemaQuantifierBindings,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CftSchemaQuantifierBindings {
    Single { binding: String },
    Array { item: String, index: String },
    Dict { key: String, value: String },
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
    FormattedString(Vec<CftSchemaCheckFormatSegment>),
    Name(String),
    Field {
        expr: Box<CftSchemaCheckExpr>,
        name: FieldName,
    },
    SafeField {
        expr: Box<CftSchemaCheckExpr>,
        name: FieldName,
    },
    Index {
        expr: Box<CftSchemaCheckExpr>,
        index: Box<CftSchemaCheckExpr>,
    },
    SafeIndex {
        expr: Box<CftSchemaCheckExpr>,
        index: Box<CftSchemaCheckExpr>,
    },
    Coalesce {
        lhs: Box<CftSchemaCheckExpr>,
        rhs: Box<CftSchemaCheckExpr>,
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
    Type(TypeName),
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CftEnum {
    pub module: ModuleId,
    pub name: EnumName,
    pub variants: Vec<CftEnumVariant>,
    pub(crate) variant_by_name: BTreeMap<EnumVariantName, usize>,
    pub(crate) variant_by_value: BTreeMap<i64, usize>,
    pub is_flag: bool,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CftEnumVariant {
    pub name: EnumVariantName,
    pub value: i64,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CftDimension {
    pub name: DimensionName,
    pub variants: Vec<crate::VariantName>,
    pub(crate) variant_by_name: BTreeMap<crate::VariantName, usize>,
    pub fields: Vec<Arc<CftField>>,
}

impl CftDimension {
    #[must_use]
    pub fn variant(&self, name: &str) -> Option<&crate::VariantName> {
        self.variant_by_name
            .get(name)
            .and_then(|index| self.variants.get(*index))
    }
}
