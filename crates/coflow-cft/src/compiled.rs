mod compiler;
mod support;
mod type_checker;

use self::compiler::SchemaCompiler;
use crate::module_id::ModuleId;
use crate::module_set::CftModuleSet;
use crate::error::CftDiagnostics;
use crate::span::Span;
use crate::{
    BucketName, ConstName, DimensionName, EnumName, EnumVariantName, FieldName, TypeName,
};
use coflow_structure::{StructuralBudget, StructuralLimits};
use std::collections::BTreeMap;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct CftCompileOptions {
    pub structural_limits: StructuralLimits,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CftConst {
    pub module: ModuleId,
    pub name: ConstName,
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
pub enum CftSchemaTypeRef {
    Int,
    Float,
    Bool,
    String,
    Object(TypeName),
    Enum(EnumName),
    RecordRef(TypeName),
    Array(Box<CftSchemaTypeRef>),
    Dict(Box<CftSchemaTypeRef>, Box<CftSchemaTypeRef>),
    Nullable(Box<CftSchemaTypeRef>),
}

impl CftSchemaTypeRef {
    #[must_use]
    pub const fn is_nullable(&self) -> bool {
        matches!(self, Self::Nullable(_))
    }

    #[must_use]
    pub fn non_nullable(&self) -> &Self {
        match self {
            Self::Nullable(inner) => inner.non_nullable(),
            other => other,
        }
    }

    #[must_use]
    pub fn display_label(&self) -> String {
        format_schema_type_ref(self)
    }
}

#[must_use]
pub fn format_schema_type_ref(ty: &CftSchemaTypeRef) -> String {
    match ty {
        CftSchemaTypeRef::Int => "int".to_string(),
        CftSchemaTypeRef::Float => "float".to_string(),
        CftSchemaTypeRef::Bool => "bool".to_string(),
        CftSchemaTypeRef::String => "string".to_string(),
        CftSchemaTypeRef::Object(name) => name.to_string(),
        CftSchemaTypeRef::Enum(name) => name.to_string(),
        CftSchemaTypeRef::RecordRef(name) => format!("&{name}"),
        CftSchemaTypeRef::Array(inner) => format!("[{}]", format_schema_type_ref(inner)),
        CftSchemaTypeRef::Dict(key, value) => {
            format!(
                "{{{}: {}}}",
                format_schema_type_ref(key),
                format_schema_type_ref(value)
            )
        }
        CftSchemaTypeRef::Nullable(inner) => format!("{}?", format_schema_type_ref(inner)),
    }
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
    pub ty_ref: CftSchemaTypeRef,
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
pub struct CftEnum {
    pub module: ModuleId,
    pub name: EnumName,
    pub variants: Vec<CftEnumVariant>,
    pub(crate) variant_by_name: BTreeMap<EnumVariantName, usize>,
    pub(crate) variant_by_value: BTreeMap<i64, usize>,
    pub is_flag: bool,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
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

#[derive(Debug, Clone, Default)]
pub(crate) struct CompiledSchema {
    pub(crate) consts: BTreeMap<ConstName, CftConst>,
    pub(crate) types: BTreeMap<TypeName, CftType>,
    pub(crate) enums: BTreeMap<EnumName, CftEnum>,
}

pub(crate) fn compile_module_set(
    modules: &CftModuleSet,
    options: CftCompileOptions,
) -> Result<(CompiledSchema, StructuralBudget), CftDiagnostics> {
    let mut compiler = SchemaCompiler::new(modules, options);
    let compiled = compiler.compile()?;
    Ok((compiled, compiler.budget))
}
