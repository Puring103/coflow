use super::CompiledSchema;
use crate::{
    CftSchemaCheckBlock, CftSchemaCheckExpr, CftSchemaCheckExprKind, CftSchemaCheckStmt,
};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn dimension_checks_for_type(
    schema: &CompiledSchema,
    type_name: &str,
) -> BTreeMap<String, CftSchemaCheckBlock> {
    let Some(check) = schema.type_meta(type_name).and_then(|meta| meta.check.as_ref()) else {
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

struct DimensionCheckAnalyzer<'a> {
    schema: &'a CompiledSchema,
    current_type: &'a str,
    scopes: Vec<BTreeSet<String>>,
}

impl<'a> DimensionCheckAnalyzer<'a> {
    fn new(schema: &'a CompiledSchema, current_type: &'a str) -> Self {
        Self {
            schema,
            current_type,
            scopes: Vec::new(),
        }
    }

    fn stmt_dimensions(&mut self, stmt: &CftSchemaCheckStmt) -> BTreeSet<String> {
        match stmt {
            CftSchemaCheckStmt::Expr(expr) => self.expr_dimensions(expr),
            CftSchemaCheckStmt::Quantifier {
                binding,
                collection,
                body,
                ..
            } => {
                let mut out = self.expr_dimensions(collection);
                self.scopes.push(BTreeSet::from([binding.clone()]));
                for stmt in body {
                    out.extend(self.stmt_dimensions(stmt));
                }
                let _ = self.scopes.pop();
                out
            }
            CftSchemaCheckStmt::When {
                condition, body, ..
            } => {
                let mut out = self.expr_dimensions(condition);
                for stmt in body {
                    out.extend(self.stmt_dimensions(stmt));
                }
                out
            }
        }
    }

    fn expr_dimensions(&mut self, expr: &CftSchemaCheckExpr) -> BTreeSet<String> {
        match &expr.kind {
            CftSchemaCheckExprKind::Int(_)
            | CftSchemaCheckExprKind::Float(_)
            | CftSchemaCheckExprKind::Bool(_)
            | CftSchemaCheckExprKind::Null
            | CftSchemaCheckExprKind::String(_) => BTreeSet::new(),
            CftSchemaCheckExprKind::Name(name) => self.name_dimensions(name),
            CftSchemaCheckExprKind::Field { expr, .. }
            | CftSchemaCheckExprKind::Is { expr, .. }
            | CftSchemaCheckExprKind::Unary { expr, .. } => self.expr_dimensions(expr),
            CftSchemaCheckExprKind::Index { expr, index } => {
                let mut out = self.expr_dimensions(expr);
                out.extend(self.expr_dimensions(index));
                out
            }
            CftSchemaCheckExprKind::Call { args, .. } => self.args_dimensions(args),
            CftSchemaCheckExprKind::MethodCall { receiver, args, .. } => {
                let mut out = self.expr_dimensions(receiver);
                out.extend(self.args_dimensions(args));
                out
            }
            CftSchemaCheckExprKind::BinOp { lhs, rhs, .. } => {
                let mut out = self.expr_dimensions(lhs);
                out.extend(self.expr_dimensions(rhs));
                out
            }
            CftSchemaCheckExprKind::CmpChain { first, rest } => {
                let mut out = self.expr_dimensions(first);
                for (_, expr) in rest {
                    out.extend(self.expr_dimensions(expr));
                }
                out
            }
        }
    }

    fn name_dimensions(&self, name: &str) -> BTreeSet<String> {
        if self
            .scopes
            .iter()
            .rev()
            .any(|scope| scope.contains(name))
        {
            return BTreeSet::new();
        }
        self.schema
            .dimension_field(self.current_type, name)
            .map(|field| BTreeSet::from([field.dimension.clone()]))
            .unwrap_or_default()
    }

    fn args_dimensions(&mut self, args: &[CftSchemaCheckExpr]) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        for arg in args {
            out.extend(self.expr_dimensions(arg));
        }
        out
    }
}
