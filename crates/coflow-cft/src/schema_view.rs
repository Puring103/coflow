use crate::{
    CftConstValue, CftContainer, CftSchemaCheckBlock, CftSchemaCheckExpr, CftSchemaCheckExprKind,
    CftSchemaCheckStmt, CftSchemaEnum, CftSchemaType, CftSchemaTypeRef,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone)]
pub struct CftSchemaView {
    pub consts: BTreeMap<String, CftConstValue>,
    pub types: BTreeMap<String, CftTypeMeta>,
    pub enums: BTreeMap<String, CftEnumMeta>,
}

impl CftSchemaView {
    #[must_use]
    pub fn new(schema: &CftContainer) -> Self {
        let consts = schema
            .module_ids()
            .filter_map(|id| schema.schema(id))
            .flat_map(|module| module.consts.iter())
            .map(|schema_const| (schema_const.name.clone(), schema_const.value.clone()))
            .collect::<BTreeMap<_, _>>();

        let enums = schema
            .all_enums()
            .map(|schema_enum| {
                (
                    schema_enum.name.clone(),
                    CftEnumMeta::from_schema(schema_enum),
                )
            })
            .collect::<BTreeMap<_, _>>();

        let types = schema
            .all_types()
            .map(|schema_type| {
                let meta = CftTypeMeta::from_schema(schema_type);
                (meta.name.clone(), meta)
            })
            .collect::<BTreeMap<_, _>>();

        let mut view = Self {
            consts,
            types,
            enums,
        };
        view.populate_dimension_checks();
        view
    }

    fn populate_dimension_checks(&mut self) {
        let names = self.types.keys().cloned().collect::<Vec<_>>();
        for name in &names {
            let checks = self.dimension_checks_for_type(name);
            if let Some(meta) = self.types.get_mut(name) {
                meta.dimension_checks = checks;
            }
        }
        // Merge ancestor dimension checks downward so child types inherit them.
        // Iterate in topological order (parents before children) by walking the
        // ancestor chain; a simple pass over all names is sufficient because
        // inheritance cycles are already rejected by the compiler.
        for name in &names {
            let mut chain: Vec<String> = Vec::new();
            let mut current = self
                .types
                .get(name.as_str())
                .and_then(|m| m.parent.clone());
            while let Some(parent_name) = current {
                chain.push(parent_name.clone());
                current = self
                    .types
                    .get(parent_name.as_str())
                    .and_then(|m| m.parent.clone());
            }
            // Collect parent dimension checks (outermost ancestor first).
            chain.reverse();
            let mut merged: BTreeMap<String, CftSchemaCheckBlock> = BTreeMap::new();
            for ancestor in &chain {
                if let Some(meta) = self.types.get(ancestor.as_str()) {
                    for (dim, block) in &meta.dimension_checks {
                        merged.entry(dim.clone()).or_insert_with(|| block.clone());
                    }
                }
            }
            if let Some(meta) = self.types.get_mut(name.as_str()) {
                for (dim, block) in merged {
                    meta.dimension_checks.entry(dim).or_insert(block);
                }
            }
        }
    }

    fn dimension_checks_for_type(&self, type_name: &str) -> BTreeMap<String, CftSchemaCheckBlock> {
        let Some(check) = self
            .types
            .get(type_name)
            .and_then(|meta| meta.check.as_ref())
        else {
            return BTreeMap::new();
        };
        let mut by_dimension: BTreeMap<String, Vec<CftSchemaCheckStmt>> = BTreeMap::new();
        let mut analyzer = DimensionCheckAnalyzer::new(self, type_name);
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

    #[must_use]
    pub fn is_assignable(&self, actual_type: &str, expected_type: &str) -> bool {
        let mut current = Some(actual_type);
        while let Some(name) = current {
            if name == expected_type {
                return true;
            }
            current = self
                .types
                .get(name)
                .and_then(|meta| meta.parent.as_deref());
        }
        false
    }

    #[must_use]
    pub fn enum_variant_value(&self, enum_name: &str, variant: &str) -> Option<i64> {
        self.enums
            .get(enum_name)
            .and_then(|meta| meta.variants.get(variant))
            .copied()
    }

    #[must_use]
    pub fn enum_value_from_int(&self, enum_name: &str, value: i64) -> Option<CftEnumValueMeta> {
        let meta = self.enums.get(enum_name)?;
        meta.variants
            .iter()
            .find(|(_, variant_value)| **variant_value == value)
            .map(|(variant, variant_value)| CftEnumValueMeta {
                enum_name: enum_name.to_string(),
                variant: Some(variant.clone()),
                value: *variant_value,
            })
    }

    #[must_use]
    pub fn checks_for_actual(
        &self,
        actual_type: &str,
        dimension: Option<&str>,
    ) -> Vec<CftSchemaCheckBlock> {
        if let Some(dimension) = dimension {
            let mut chain = Vec::new();
            let mut current = Some(actual_type);
            while let Some(name) = current {
                let Some(meta) = self.types.get(name) else {
                    break;
                };
                chain.push(meta);
                current = meta.parent.as_deref();
            }
            chain.reverse();
            return chain
                .into_iter()
                .filter_map(|meta| meta.dimension_checks.get(dimension).cloned())
                .collect();
        }
        let mut chain = Vec::new();
        let mut current = Some(actual_type);
        while let Some(name) = current {
            let Some(meta) = self.types.get(name) else {
                break;
            };
            chain.push(meta);
            current = meta.parent.as_deref();
        }
        chain.reverse();
        chain
            .into_iter()
            .filter_map(|meta| meta.check.clone())
            .collect()
    }

    #[must_use]
    pub fn field_type(&self, actual_type: &str, field_name: &str) -> Option<&CftSchemaTypeRef> {
        self.types
            .get(actual_type)
            .and_then(|meta| meta.fields.get(field_name))
    }

    #[must_use]
    pub fn dimension_field(
        &self,
        actual_type: &str,
        field_name: &str,
    ) -> Option<&CftDimensionFieldMeta> {
        self.types
            .get(actual_type)
            .and_then(|meta| meta.dimension_fields.get(field_name))
    }
}

#[derive(Debug, Clone)]
pub struct CftTypeMeta {
    pub name: String,
    pub parent: Option<String>,
    pub check: Option<CftSchemaCheckBlock>,
    pub dimension_checks: BTreeMap<String, CftSchemaCheckBlock>,
    pub fields: BTreeMap<String, CftSchemaTypeRef>,
    pub dimension_fields: BTreeMap<String, CftDimensionFieldMeta>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CftDimensionFieldMeta {
    pub dimension: String,
}

impl CftTypeMeta {
    fn from_schema(schema_type: &CftSchemaType) -> Self {
        let dimension_fields = schema_type
            .all_fields
            .iter()
            .filter_map(|field| {
                let dimension = field.dimension.as_ref().map(|d| d.kind.name())?;
                Some((
                    field.name.clone(),
                    CftDimensionFieldMeta {
                        dimension: dimension.to_string(),
                    },
                ))
            })
            .collect();
        Self {
            name: schema_type.name.clone(),
            parent: schema_type.parent.clone(),
            check: schema_type.check.clone(),
            dimension_checks: BTreeMap::new(),
            fields: schema_type
                .all_fields
                .iter()
                .map(|field| (field.name.clone(), field.ty_ref.clone()))
                .collect(),
            dimension_fields,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CftEnumValueMeta {
    pub enum_name: String,
    pub variant: Option<String>,
    pub value: i64,
}

#[derive(Debug, Clone)]
pub struct CftEnumMeta {
    pub variants: BTreeMap<String, i64>,
}

impl CftEnumMeta {
    fn from_schema(schema_enum: &CftSchemaEnum) -> Self {
        Self {
            variants: schema_enum
                .variants
                .iter()
                .map(|variant| (variant.name.clone(), variant.value))
                .collect(),
        }
    }
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
