use super::CftValueType;
use crate::module::ModuleId;
use crate::source::Span;
use crate::{
    BucketName, CheckName, ConstName, DimensionName, EnumName, EnumVariantName, FieldName, TypeName,
};
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CheckStatementId(u32);

impl CheckStatementId {
    #[must_use]
    pub(in crate::schema) const fn new(value: u32) -> Self {
        Self(value)
    }

    #[must_use]
    pub(in crate::schema) const fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum CheckOwner {
    Type(TypeName),
    Project(CheckName),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct CheckField {
    pub owner: TypeName,
    pub field: FieldName,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum CheckDependency {
    Field(CheckField),
    RecordSet(TypeName),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum CheckDependencyLocality {
    Local,
    CrossRecord,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct CheckStatementDependencies {
    uses: BTreeMap<CheckDependency, CheckDependencyLocality>,
}

impl CheckStatementDependencies {
    pub(crate) fn insert(
        &mut self,
        dependency: CheckDependency,
        locality: CheckDependencyLocality,
    ) {
        self.uses
            .entry(dependency)
            .and_modify(|existing| *existing = (*existing).max(locality))
            .or_insert(locality);
    }

    pub(crate) fn dependencies(&self) -> impl Iterator<Item = &CheckDependency> {
        self.uses.keys()
    }

    pub(crate) fn cross_record_dependencies(&self) -> impl Iterator<Item = &CheckDependency> {
        self.uses.iter().filter_map(|(dependency, locality)| {
            (*locality == CheckDependencyLocality::CrossRecord).then_some(dependency)
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckStatementInfo {
    pub id: CheckStatementId,
    pub owner: CheckOwner,
    pub root_index: usize,
    pub dependencies: BTreeSet<CheckDependency>,
    pub dimensions: BTreeSet<DimensionName>,
}

#[derive(Debug, Clone)]
pub struct CftSchemaSource {
    pub path: PathBuf,
    pub source: Arc<str>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CftConst {
    pub module: ModuleId,
    pub name: ConstName,
    pub value_type: CftValueType,
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
    FormattedString(String),
    Function(String),
    Enum {
        enum_name: EnumName,
        variant: EnumVariantName,
        value: i64,
    },
    OptionNone,
    OptionSome(Box<CftConstValue>),
    ResultOk(Box<CftConstValue>),
    ResultErr(Box<CftConstValue>),
    Array(Vec<CftConstValue>),
    Dictionary(Vec<(CftConstValue, CftConstValue)>),
    Object {
        type_name: TypeName,
        fields: Vec<(FieldName, CftConstValue)>,
    },
    RecordReference {
        type_name: TypeName,
        key: String,
    },
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
    pub is_host: bool,
    pub id_as_enum: Option<EnumName>,
    pub annotations: Vec<CftAnnotation>,
    pub display: Option<CftDisplayMetadata>,
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
    pub annotations: Vec<CftAnnotation>,
    pub display: Option<CftDisplayMetadata>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CftSchemaDefaultValue {
    OptionNone,
    OptionSome(Box<CftSchemaDefaultValue>),
    ResultOk(Box<CftSchemaDefaultValue>),
    ResultErr(Box<CftSchemaDefaultValue>),
    Int(i64),
    Float(f64),
    Bool(bool),
    String(String),
    FormattedString(String),
    Function(String),
    Enum {
        enum_name: EnumName,
        variant: EnumVariantName,
        value: i64,
    },
    EmptyArray,
    EmptyObject,
    Array(Vec<CftSchemaDefaultValue>),
    Dictionary(Vec<(CftSchemaDefaultValue, CftSchemaDefaultValue)>),
    Object {
        type_name: TypeName,
        fields: Vec<(FieldName, CftSchemaDefaultValue)>,
    },
    RecordReference {
        type_name: TypeName,
        key: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct CftSchemaCheckBlock {
    pub stmts: Vec<CftSchemaCheckStmt>,
    pub span: Span,
    pub(crate) dimension_statements: BTreeMap<DimensionName, Vec<usize>>,
    pub(crate) statement_dependencies: Vec<CheckStatementDependencies>,
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
    String(String),
    FormattedString(Vec<CftSchemaCheckFormatSegment>),
    Name(String),
    Records {
        type_name: TypeName,
    },
    Field {
        expr: Box<CftSchemaCheckExpr>,
        name: FieldName,
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
    Type(TypeName),
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
    pub annotations: Vec<CftAnnotation>,
    pub display: Option<CftDisplayMetadata>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CftEnumVariant {
    pub name: EnumVariantName,
    pub value: i64,
    pub annotations: Vec<CftAnnotation>,
    pub display: Option<CftDisplayMetadata>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CftAnnotation {
    pub name: String,
    pub arguments: Vec<CftAnnotationValue>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CftAnnotationValue {
    Name(String),
    String(String),
    Int(i64),
    Float(f64),
    Bool(bool),
}

/// Human-facing metadata which never changes schema identity or stored data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CftDisplayMetadata {
    pub label: Option<String>,
    pub description: Option<String>,
}

impl CftDisplayMetadata {
    #[must_use]
    pub fn summary(&self) -> Option<&str> {
        self.description.as_deref().or(self.label.as_deref())
    }
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

    #[must_use]
    pub fn variant_index(&self, name: &str) -> Option<usize> {
        self.variant_by_name.get(name).copied()
    }
}
