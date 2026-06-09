use super::{
    CftAnnotation, CftAnnotationValue, CftConstValue, CftSchemaBinOp, CftSchemaCheckBlock,
    CftSchemaCheckExpr, CftSchemaCheckExprKind, CftSchemaCheckStmt, CftSchemaCmpOp,
    CftSchemaQuantifierKind, CftSchemaTypePredicate, CftSchemaUnaryOp,
};
use crate::ast::{
    Annotation, AnnotationArg, BinOp, CheckExpr, CheckExprKind, CheckStmt, CmpOp, ConstLiteral,
    EnumDef, TypeDef, TypePredicate, TypeRef, TypeRefKind, UnaryOp,
};
use crate::container::ModuleId;
use crate::span::Span;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone)]
pub(super) struct ConstInfo<'a> {
    pub(super) module: ModuleId,
    pub(super) def: &'a crate::ast::ConstDef,
    pub(super) value: CftConstValue,
}

#[derive(Debug, Clone)]
pub(super) struct TypeInfo<'a> {
    pub(super) module: ModuleId,
    pub(super) def: &'a TypeDef,
}

#[derive(Debug, Clone)]
pub(super) struct EnumInfo<'a> {
    pub(super) module: ModuleId,
    pub(super) def: &'a EnumDef,
    pub(super) variants: BTreeSet<String>,
    /// `value -> (declaring module, span of the originating variant)`. Used
    /// only for duplicate-value diagnostics during schema validation.
    pub(super) values: BTreeMap<i64, (ModuleId, Span)>,
    /// `variant name -> resolved integer value`, populated once during
    /// `validate_enums` so later passes (default-value lowering, schema
    /// construction) don't need to re-walk the variant list.
    pub(super) values_by_name: BTreeMap<String, i64>,
    pub(super) is_flag: bool,
}

#[derive(Debug, Clone)]
pub(super) struct FieldInfo {
    pub(super) check_ty: Ty,
}

#[derive(Debug, Clone)]
pub(super) struct FieldOrigin {
    pub(super) module: ModuleId,
    pub(super) span: Span,
}

#[derive(Debug, Clone)]
pub(super) struct Symbol {
    pub(super) kind: SymbolKind,
    pub(super) module: ModuleId,
    pub(super) span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SymbolKind {
    Const,
    Type,
    Enum,
}

pub(super) fn is_reserved_identifier(name: &str) -> bool {
    matches!(
        name,
        "_" | "const"
            | "enum"
            | "type"
            | "abstract"
            | "sealed"
            | "check"
            | "when"
            | "all"
            | "any"
            | "none"
            | "in"
            | "is"
            | "true"
            | "false"
            | "null"
            | "int"
            | "float"
            | "bool"
            | "string"
            | "len"
            | "contains"
            | "unique"
            | "min"
            | "max"
            | "sum"
            | "keys"
            | "values"
            | "matches"
            | "if"
            | "else"
            | "match"
            | "case"
            | "for"
            | "while"
            | "let"
            | "module"
            | "import"
            | "export"
            | "from"
            | "as"
            | "use"
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AnnotationTarget {
    Type,
    Enum,
    Field,
}

#[derive(Debug, Clone)]
pub(super) struct AnnotationSpec {
    pub(super) targets: &'static [AnnotationTarget],
    args: AnnotationArgs,
}

impl AnnotationSpec {
    pub(super) fn for_name(name: &str) -> Option<Self> {
        Some(match name {
            "struct" => Self {
                targets: &[AnnotationTarget::Type],
                args: AnnotationArgs::None,
            },
            "flag" => Self {
                targets: &[AnnotationTarget::Enum],
                args: AnnotationArgs::None,
            },
            "id" | "index" => Self {
                targets: &[AnnotationTarget::Field],
                args: AnnotationArgs::None,
            },
            "ref" => Self {
                targets: &[AnnotationTarget::Field],
                args: AnnotationArgs::OneName,
            },
            "display" => Self {
                targets: &[
                    AnnotationTarget::Type,
                    AnnotationTarget::Enum,
                    AnnotationTarget::Field,
                ],
                args: AnnotationArgs::OneString,
            },
            "deprecated" => Self {
                targets: &[
                    AnnotationTarget::Type,
                    AnnotationTarget::Enum,
                    AnnotationTarget::Field,
                ],
                args: AnnotationArgs::None,
            },
            _ => return None,
        })
    }

    pub(super) fn args_valid(&self, annotation: &Annotation) -> bool {
        match self.args {
            AnnotationArgs::None => annotation.args.is_empty(),
            AnnotationArgs::OneName => {
                matches!(annotation.args.as_slice(), [AnnotationArg::Name(_)])
            }
            AnnotationArgs::OneString => {
                matches!(annotation.args.as_slice(), [AnnotationArg::String(_, _)])
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum AnnotationArgs {
    None,
    OneName,
    OneString,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Ty {
    Int,
    Float,
    Bool,
    String,
    Null,
    Type(String),
    Enum(String),
    EnumNamespace(String),
    Array(Box<Ty>),
    Dict(Box<Ty>, Box<Ty>),
    Nullable(Box<Ty>),
    Entry(Box<Ty>, Box<Ty>),
    EmptyArray,
    EmptyObject,
    Unknown,
}

impl Ty {
    pub(super) fn from_const(value: &CftConstValue) -> Self {
        match value {
            CftConstValue::Int(_) => Self::Int,
            CftConstValue::Float(_) => Self::Float,
            CftConstValue::Bool(_) => Self::Bool,
            CftConstValue::String(_) => Self::String,
        }
    }

    pub(super) fn is_nullable(&self) -> bool {
        matches!(self, Self::Nullable(_))
    }
}

pub(super) fn unwrap_nullable(ty: &Ty) -> &Ty {
    match ty {
        Ty::Nullable(inner) => inner,
        other => other,
    }
}

pub(super) fn const_value(value: &ConstLiteral) -> CftConstValue {
    match value {
        ConstLiteral::Int(value, _) => CftConstValue::Int(*value),
        ConstLiteral::Float(value, _) => CftConstValue::Float(*value),
        ConstLiteral::Bool(value, _) => CftConstValue::Bool(*value),
        ConstLiteral::String(value, _) => CftConstValue::String(value.clone()),
    }
}

pub(super) fn has_annotation(annotations: &[Annotation], name: &str) -> bool {
    find_annotation(annotations, name).is_some()
}

pub(super) fn find_annotation<'a>(
    annotations: &'a [Annotation],
    name: &str,
) -> Option<&'a Annotation> {
    annotations
        .iter()
        .find(|annotation| annotation.name == name)
}

pub(super) fn convert_annotations(annotations: &[Annotation]) -> Vec<CftAnnotation> {
    annotations
        .iter()
        .map(|annotation| CftAnnotation {
            name: annotation.name.clone(),
            args: annotation
                .args
                .iter()
                .map(|arg| match arg {
                    AnnotationArg::Name(name) => CftAnnotationValue::Name(name.name.clone()),
                    AnnotationArg::String(value, _) => CftAnnotationValue::String(value.clone()),
                    AnnotationArg::Int(value, _) => CftAnnotationValue::Int(*value),
                    AnnotationArg::Float(value, _) => CftAnnotationValue::Float(*value),
                    AnnotationArg::Bool(value, _) => CftAnnotationValue::Bool(*value),
                    AnnotationArg::Null(_) => CftAnnotationValue::Null,
                })
                .collect(),
        })
        .collect()
}

pub(super) fn convert_check_block(check: &crate::ast::CheckBlock) -> CftSchemaCheckBlock {
    CftSchemaCheckBlock {
        stmts: check.stmts.iter().map(convert_check_stmt).collect(),
        span: check.span,
    }
}

fn convert_check_stmt(stmt: &CheckStmt) -> CftSchemaCheckStmt {
    match stmt {
        CheckStmt::Expr(expr) => CftSchemaCheckStmt::Expr(convert_check_expr(expr)),
        CheckStmt::Quantifier {
            kind,
            binding,
            collection,
            body,
            span,
        } => CftSchemaCheckStmt::Quantifier {
            kind: match kind {
                crate::ast::QuantifierKind::All => CftSchemaQuantifierKind::All,
                crate::ast::QuantifierKind::Any => CftSchemaQuantifierKind::Any,
                crate::ast::QuantifierKind::None => CftSchemaQuantifierKind::None,
            },
            binding: binding.name.clone(),
            collection: convert_check_expr(collection),
            body: body.iter().map(convert_check_stmt).collect(),
            span: *span,
        },
        CheckStmt::When {
            condition,
            body,
            span,
        } => CftSchemaCheckStmt::When {
            condition: convert_check_expr(condition),
            body: body.iter().map(convert_check_stmt).collect(),
            span: *span,
        },
    }
}

fn convert_check_expr(expr: &CheckExpr) -> CftSchemaCheckExpr {
    CftSchemaCheckExpr {
        kind: match &expr.kind {
            CheckExprKind::Int(value) => CftSchemaCheckExprKind::Int(*value),
            CheckExprKind::Float(value) => CftSchemaCheckExprKind::Float(*value),
            CheckExprKind::Bool(value) => CftSchemaCheckExprKind::Bool(*value),
            CheckExprKind::Null => CftSchemaCheckExprKind::Null,
            CheckExprKind::String(value) => CftSchemaCheckExprKind::String(value.clone()),
            CheckExprKind::Name(name) => CftSchemaCheckExprKind::Name(name.clone()),
            CheckExprKind::Field { expr: inner, name } => CftSchemaCheckExprKind::Field {
                expr: Box::new(convert_check_expr(inner)),
                name: name.name.clone(),
            },
            CheckExprKind::Index { expr: inner, index } => CftSchemaCheckExprKind::Index {
                expr: Box::new(convert_check_expr(inner)),
                index: Box::new(convert_check_expr(index)),
            },
            CheckExprKind::Is {
                expr: inner,
                predicate,
            } => CftSchemaCheckExprKind::Is {
                expr: Box::new(convert_check_expr(inner)),
                predicate: match predicate {
                    TypePredicate::Type(name) => CftSchemaTypePredicate::Type(name.name.clone()),
                    TypePredicate::Null(_) => CftSchemaTypePredicate::Null,
                },
            },
            CheckExprKind::Call { name, args } => CftSchemaCheckExprKind::Call {
                name: name.name.clone(),
                args: args.iter().map(convert_check_expr).collect(),
            },
            CheckExprKind::BinOp { op, lhs, rhs } => CftSchemaCheckExprKind::BinOp {
                op: convert_bin_op(*op),
                lhs: Box::new(convert_check_expr(lhs)),
                rhs: Box::new(convert_check_expr(rhs)),
            },
            CheckExprKind::Unary { op, expr: inner } => CftSchemaCheckExprKind::Unary {
                op: match op {
                    UnaryOp::Not => CftSchemaUnaryOp::Not,
                    UnaryOp::BitNot => CftSchemaUnaryOp::BitNot,
                    UnaryOp::Neg => CftSchemaUnaryOp::Neg,
                },
                expr: Box::new(convert_check_expr(inner)),
            },
            CheckExprKind::CmpChain { first, rest } => CftSchemaCheckExprKind::CmpChain {
                first: Box::new(convert_check_expr(first)),
                rest: rest
                    .iter()
                    .map(|(op, rhs)| (convert_cmp_op(*op), convert_check_expr(rhs)))
                    .collect(),
            },
        },
        span: expr.span,
    }
}

fn convert_bin_op(op: BinOp) -> CftSchemaBinOp {
    match op {
        BinOp::Or => CftSchemaBinOp::Or,
        BinOp::And => CftSchemaBinOp::And,
        BinOp::BitOr => CftSchemaBinOp::BitOr,
        BinOp::BitXor => CftSchemaBinOp::BitXor,
        BinOp::BitAnd => CftSchemaBinOp::BitAnd,
        BinOp::Add => CftSchemaBinOp::Add,
        BinOp::Sub => CftSchemaBinOp::Sub,
        BinOp::Shl => CftSchemaBinOp::Shl,
        BinOp::Shr => CftSchemaBinOp::Shr,
        BinOp::Mul => CftSchemaBinOp::Mul,
        BinOp::Div => CftSchemaBinOp::Div,
        BinOp::IntDiv => CftSchemaBinOp::IntDiv,
        BinOp::Mod => CftSchemaBinOp::Mod,
        BinOp::Pow => CftSchemaBinOp::Pow,
    }
}

fn convert_cmp_op(op: CmpOp) -> CftSchemaCmpOp {
    match op {
        CmpOp::Eq => CftSchemaCmpOp::Eq,
        CmpOp::Ne => CftSchemaCmpOp::Ne,
        CmpOp::Lt => CftSchemaCmpOp::Lt,
        CmpOp::Le => CftSchemaCmpOp::Le,
        CmpOp::Gt => CftSchemaCmpOp::Gt,
        CmpOp::Ge => CftSchemaCmpOp::Ge,
    }
}

pub(super) fn format_type_ref(ty: &TypeRef) -> String {
    match &ty.kind {
        TypeRefKind::Int => "int".to_string(),
        TypeRefKind::Float => "float".to_string(),
        TypeRefKind::Bool => "bool".to_string(),
        TypeRefKind::String => "string".to_string(),
        TypeRefKind::Named(name) => name.clone(),
        TypeRefKind::Array(inner) => format!("[{}]", format_type_ref(inner)),
        TypeRefKind::Dict(key, value) => {
            format!("{{{}: {}}}", format_type_ref(key), format_type_ref(value))
        }
        TypeRefKind::Nullable(inner) => format!("{}?", format_type_ref(inner)),
    }
}

pub(super) fn build_schema_type_ref(ty: &TypeRef) -> super::CftSchemaTypeRef {
    use super::CftSchemaTypeRef;
    match &ty.kind {
        TypeRefKind::Int => CftSchemaTypeRef::Int,
        TypeRefKind::Float => CftSchemaTypeRef::Float,
        TypeRefKind::Bool => CftSchemaTypeRef::Bool,
        TypeRefKind::String => CftSchemaTypeRef::String,
        TypeRefKind::Named(name) => CftSchemaTypeRef::Named(name.clone()),
        TypeRefKind::Array(inner) => {
            CftSchemaTypeRef::Array(Box::new(build_schema_type_ref(inner)))
        }
        TypeRefKind::Dict(key, value) => CftSchemaTypeRef::Dict(
            Box::new(build_schema_type_ref(key)),
            Box::new(build_schema_type_ref(value)),
        ),
        TypeRefKind::Nullable(inner) => {
            CftSchemaTypeRef::Nullable(Box::new(build_schema_type_ref(inner)))
        }
    }
}

pub(super) fn is_valid_dict_key(ty: &Ty) -> bool {
    matches!(ty, Ty::Int | Ty::String | Ty::Enum(_) | Ty::Unknown)
}

pub(super) fn is_string_or_int(ty: &Ty, allow_nullable: bool) -> bool {
    match ty {
        Ty::String | Ty::Int | Ty::Unknown => true,
        Ty::Nullable(inner) if allow_nullable => matches!(inner.as_ref(), Ty::String | Ty::Int),
        _ => false,
    }
}

pub(super) fn is_indexable_field_type(ty: &Ty) -> bool {
    matches!(ty, Ty::String | Ty::Int | Ty::Enum(_) | Ty::Unknown)
}

pub(super) fn types_assignable(expected: &Ty, actual: &Ty) -> bool {
    if matches!(expected, Ty::Unknown) || matches!(actual, Ty::Unknown) {
        return true;
    }
    match (expected, actual) {
        (Ty::Nullable(inner), Ty::Null) => !matches!(inner.as_ref(), Ty::Unknown),
        (Ty::Nullable(inner), other) => types_assignable(inner, other),
        (Ty::Array(_), Ty::EmptyArray) | (Ty::Dict(_, _), Ty::EmptyObject) => true,
        (Ty::Enum(left), Ty::Enum(right)) | (Ty::Type(left), Ty::Type(right)) => left == right,
        _ => expected == actual,
    }
}

pub(super) fn types_comparable(left: &Ty, right: &Ty) -> bool {
    if matches!(left, Ty::Unknown) || matches!(right, Ty::Unknown) {
        return true;
    }
    if matches!((left, right), (Ty::Null, Ty::Null)) {
        return true;
    }
    if matches!(
        (left, right),
        (Ty::Null, Ty::Nullable(_)) | (Ty::Nullable(_), Ty::Null)
    ) {
        return true;
    }
    match (unwrap_nullable(left), unwrap_nullable(right)) {
        (Ty::Unknown, _)
        | (_, Ty::Unknown)
        | (Ty::Null, Ty::Null)
        | (Ty::Int, Ty::Int)
        | (Ty::Float, Ty::Float)
        | (Ty::Bool, Ty::Bool)
        | (Ty::String, Ty::String) => true,
        (Ty::Enum(left), Ty::Enum(right)) | (Ty::Type(left), Ty::Type(right)) => left == right,
        _ => false,
    }
}

pub(super) fn ordered_comparable(left: &Ty, right: &Ty) -> bool {
    match (unwrap_nullable(left), unwrap_nullable(right)) {
        (Ty::Unknown, _) | (_, Ty::Unknown) | (Ty::Int, Ty::Int) | (Ty::Float, Ty::Float) => true,
        (Ty::Enum(left), Ty::Enum(right)) => left == right,
        _ => false,
    }
}

pub(super) fn unique_supported(ty: &Ty) -> bool {
    matches!(
        unwrap_nullable(ty),
        Ty::Int | Ty::Bool | Ty::String | Ty::Enum(_)
    )
}

pub(super) fn min_max_supported(ty: &Ty) -> bool {
    matches!(unwrap_nullable(ty), Ty::Int | Ty::Float | Ty::Enum(_))
}

pub(super) fn is_i64_power_of_two(value: i64) -> bool {
    value > 0 && (value & (value - 1)) == 0
}
