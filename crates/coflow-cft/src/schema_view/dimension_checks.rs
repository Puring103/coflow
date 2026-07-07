use super::CftSchemaView;
use crate::{
    CftConstValue, CftSchemaCheckBlock, CftSchemaCheckExpr, CftSchemaCheckExprKind,
    CftSchemaCheckStmt, CftSchemaTypeRef,
};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn dimension_checks_for_type(
    schema: &CftSchemaView,
    type_name: &str,
) -> BTreeMap<String, CftSchemaCheckBlock> {
    let Some(check) = schema
        .types
        .get(type_name)
        .and_then(|meta| meta.check.as_ref())
    else {
        return BTreeMap::new();
    };
    let mut by_dimension: BTreeMap<String, Vec<CftSchemaCheckStmt>> = BTreeMap::new();
    let mut analyzer = DimensionCheckAnalyzer::new(schema, type_name);
    for stmt in &check.stmts {
        for dimension in analyzer.stmt_dimensions(stmt) {
            by_dimension
                .entry(dimension)
                .or_default()
                .push(stmt.clone());
        }
    }
    by_dimension
        .into_iter()
        .map(|(dimension, stmts)| {
            (
                dimension,
                CftSchemaCheckBlock {
                    stmts,
                    span: check.span,
                },
            )
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CheckTy {
    Int,
    Float,
    Bool,
    String,
    Null,
    Type(String),
    Enum(String),
    Array(Box<CheckTy>),
    Dict(Box<CheckTy>, Box<CheckTy>),
    Nullable(Box<CheckTy>),
    Entry(Box<CheckTy>, Box<CheckTy>),
    Unknown,
}

impl CheckTy {
    fn unwrap_nullable(&self) -> &Self {
        match self {
            Self::Nullable(inner) => inner,
            other => other,
        }
    }
}

#[derive(Debug, Clone)]
struct ExprUsage {
    ty: CheckTy,
    dimensions: BTreeSet<String>,
}

impl ExprUsage {
    fn new(ty: CheckTy) -> Self {
        Self {
            ty,
            dimensions: BTreeSet::new(),
        }
    }
}

struct DimensionCheckAnalyzer<'a> {
    schema: &'a CftSchemaView,
    current_type: String,
    scopes: Vec<BTreeMap<String, CheckTy>>,
}

impl<'a> DimensionCheckAnalyzer<'a> {
    fn new(schema: &'a CftSchemaView, current_type: &str) -> Self {
        Self {
            schema,
            current_type: current_type.to_string(),
            scopes: Vec::new(),
        }
    }

    fn stmt_dimensions(&mut self, stmt: &CftSchemaCheckStmt) -> BTreeSet<String> {
        match stmt {
            CftSchemaCheckStmt::Expr(expr) => self.expr_usage(expr).dimensions,
            CftSchemaCheckStmt::Quantifier {
                binding,
                collection,
                body,
                ..
            } => {
                let collection = self.expr_usage(collection);
                let mut out = collection.dimensions;
                let item_ty = match collection.ty.unwrap_nullable() {
                    CheckTy::Array(inner) => inner.as_ref().clone(),
                    CheckTy::Dict(key, value) => {
                        CheckTy::Entry(Box::new(key.as_ref().clone()), value.clone())
                    }
                    _ => CheckTy::Unknown,
                };
                let mut scope = BTreeMap::new();
                scope.insert(binding.clone(), item_ty);
                self.scopes.push(scope);
                for stmt in body {
                    out.extend(self.stmt_dimensions(stmt));
                }
                let _ = self.scopes.pop();
                out
            }
            CftSchemaCheckStmt::When {
                condition, body, ..
            } => {
                let mut out = self.expr_usage(condition).dimensions;
                for stmt in body {
                    out.extend(self.stmt_dimensions(stmt));
                }
                out
            }
        }
    }

    fn expr_usage(&mut self, expr: &CftSchemaCheckExpr) -> ExprUsage {
        match &expr.kind {
            CftSchemaCheckExprKind::Int(_) => ExprUsage::new(CheckTy::Int),
            CftSchemaCheckExprKind::Float(_) => ExprUsage::new(CheckTy::Float),
            CftSchemaCheckExprKind::Bool(_) => ExprUsage::new(CheckTy::Bool),
            CftSchemaCheckExprKind::Null => ExprUsage::new(CheckTy::Null),
            CftSchemaCheckExprKind::String(_) => ExprUsage::new(CheckTy::String),
            CftSchemaCheckExprKind::Name(name) => self.name_usage(name),
            CftSchemaCheckExprKind::Field { expr, name } => self.field_usage(expr, name),
            CftSchemaCheckExprKind::Index { expr, index } => {
                let target = self.expr_usage(expr);
                let index = self.expr_usage(index);
                let mut dimensions = target.dimensions;
                dimensions.extend(index.dimensions);
                let ty = match target.ty.unwrap_nullable() {
                    CheckTy::Array(inner) => inner.as_ref().clone(),
                    CheckTy::Dict(_, value) => value.as_ref().clone(),
                    _ => CheckTy::Unknown,
                };
                ExprUsage { ty, dimensions }
            }
            CftSchemaCheckExprKind::Is { expr, .. } => {
                let mut usage = self.expr_usage(expr);
                usage.ty = CheckTy::Bool;
                usage
            }
            CftSchemaCheckExprKind::Call { name, args } => self.call_usage(name, args),
            CftSchemaCheckExprKind::MethodCall {
                receiver,
                name,
                args,
            } => self.method_usage(receiver, name, args),
            CftSchemaCheckExprKind::BinOp { op: _, lhs, rhs } => {
                let lhs = self.expr_usage(lhs);
                let rhs = self.expr_usage(rhs);
                let mut dimensions = lhs.dimensions;
                dimensions.extend(rhs.dimensions);
                ExprUsage {
                    ty: CheckTy::Unknown,
                    dimensions,
                }
            }
            CftSchemaCheckExprKind::Unary { expr, .. } => {
                let mut usage = self.expr_usage(expr);
                usage.ty = CheckTy::Unknown;
                usage
            }
            CftSchemaCheckExprKind::CmpChain { first, rest } => {
                let mut usage = self.expr_usage(first);
                for (_, expr) in rest {
                    usage.dimensions.extend(self.expr_usage(expr).dimensions);
                }
                usage.ty = CheckTy::Bool;
                usage
            }
        }
    }

    fn name_usage(&self, name: &str) -> ExprUsage {
        if let Some(ty) = self
            .scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).cloned())
        {
            return ExprUsage::new(ty);
        }
        if let Some(meta) = self.schema.types.get(&self.current_type) {
            if let Some(field) = meta.dimension_fields.get(name) {
                let mut dimensions = BTreeSet::new();
                dimensions.insert(field.dimension.clone());
                return ExprUsage {
                    ty: meta
                        .fields
                        .get(name)
                        .map_or(CheckTy::Unknown, type_ref_to_check_ty),
                    dimensions,
                };
            }
            if let Some(ty) = meta.fields.get(name) {
                return ExprUsage::new(type_ref_to_check_ty(ty));
            }
        }
        if let Some(value) = self.schema.consts.get(name) {
            return ExprUsage::new(const_to_check_ty(value));
        }
        if self.schema.enums.contains_key(name) {
            return ExprUsage::new(CheckTy::Enum(name.to_string()));
        }
        ExprUsage::new(CheckTy::Unknown)
    }

    fn field_usage(&mut self, expr: &CftSchemaCheckExpr, name: &str) -> ExprUsage {
        let target = self.expr_usage(expr);
        let dimensions = target.dimensions;
        let ty = match target.ty.unwrap_nullable() {
            CheckTy::Type(type_name) => {
                if name == "id" {
                    CheckTy::String
                } else if let Some(meta) = self.schema.types.get(type_name) {
                    meta.fields
                        .get(name)
                        .map_or(CheckTy::Unknown, type_ref_to_check_ty)
                } else {
                    CheckTy::Unknown
                }
            }
            CheckTy::Entry(key, value) => match name {
                "key" => key.as_ref().clone(),
                "value" => value.as_ref().clone(),
                _ => CheckTy::Unknown,
            },
            _ => CheckTy::Unknown,
        };
        ExprUsage { ty, dimensions }
    }

    fn call_usage(&mut self, name: &str, args: &[CftSchemaCheckExpr]) -> ExprUsage {
        let arg_usages: Vec<ExprUsage> = args.iter().map(|arg| self.expr_usage(arg)).collect();
        let mut dimensions = BTreeSet::new();
        for usage in &arg_usages {
            dimensions.extend(usage.dimensions.iter().cloned());
        }
        let ty = if self.schema.enums.contains_key(name) {
            CheckTy::Enum(name.to_string())
        } else {
            match name {
                "len" => CheckTy::Int,
                "contains" | "isUnique" | "matches" => CheckTy::Bool,
                "keys" => arg_usages.first().map_or(CheckTy::Unknown, |usage| {
                    match usage.ty.unwrap_nullable() {
                        CheckTy::Dict(key, _) => CheckTy::Array(key.clone()),
                        _ => CheckTy::Unknown,
                    }
                }),
                "values" => arg_usages.first().map_or(CheckTy::Unknown, |usage| {
                    match usage.ty.unwrap_nullable() {
                        CheckTy::Dict(_, value) => CheckTy::Array(value.clone()),
                        _ => CheckTy::Unknown,
                    }
                }),
                _ => CheckTy::Unknown,
            }
        };
        ExprUsage { ty, dimensions }
    }

    fn method_usage(
        &mut self,
        receiver: &CftSchemaCheckExpr,
        name: &str,
        args: &[CftSchemaCheckExpr],
    ) -> ExprUsage {
        let receiver = self.expr_usage(receiver);
        let mut dimensions = receiver.dimensions;
        for arg in args {
            dimensions.extend(self.expr_usage(arg).dimensions);
        }
        let ty = match name {
            "len" => CheckTy::Int,
            "contains" | "isUnique" | "matches" => CheckTy::Bool,
            "keys" => match receiver.ty.unwrap_nullable() {
                CheckTy::Dict(key, _) => CheckTy::Array(key.clone()),
                _ => CheckTy::Unknown,
            },
            "values" => match receiver.ty.unwrap_nullable() {
                CheckTy::Dict(_, value) => CheckTy::Array(value.clone()),
                _ => CheckTy::Unknown,
            },
            _ => CheckTy::Unknown,
        };
        ExprUsage { ty, dimensions }
    }
}

fn type_ref_to_check_ty(ty: &CftSchemaTypeRef) -> CheckTy {
    match ty {
        CftSchemaTypeRef::Int => CheckTy::Int,
        CftSchemaTypeRef::Float => CheckTy::Float,
        CftSchemaTypeRef::Bool => CheckTy::Bool,
        CftSchemaTypeRef::String => CheckTy::String,
        CftSchemaTypeRef::Named(name) | CftSchemaTypeRef::Ref(name) => CheckTy::Type(name.clone()),
        CftSchemaTypeRef::Array(inner) => CheckTy::Array(Box::new(type_ref_to_check_ty(inner))),
        CftSchemaTypeRef::Dict(key, value) => CheckTy::Dict(
            Box::new(type_ref_to_check_ty(key)),
            Box::new(type_ref_to_check_ty(value)),
        ),
        CftSchemaTypeRef::Nullable(inner) => {
            CheckTy::Nullable(Box::new(type_ref_to_check_ty(inner)))
        }
    }
}

fn const_to_check_ty(value: &CftConstValue) -> CheckTy {
    match value {
        CftConstValue::Int(_) => CheckTy::Int,
        CftConstValue::Float(_) => CheckTy::Float,
        CftConstValue::Bool(_) => CheckTy::Bool,
        CftConstValue::String(_) => CheckTy::String,
    }
}
