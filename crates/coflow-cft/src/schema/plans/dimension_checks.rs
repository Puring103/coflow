use crate::module::ModuleId;
use crate::schema::LocatedBudgetError;
use crate::{
    CftSchemaCheckExpr, CftSchemaCheckExprKind, CftSchemaCheckStmt, CftType, DimensionName, Span,
    TypeName,
};
use coflow_structure::{StructuralBudget, StructureKind};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn dimension_checks_for_type(
    types: &BTreeMap<TypeName, CftType>,
    type_name: &TypeName,
    budget: &mut StructuralBudget,
) -> Result<BTreeMap<DimensionName, Vec<usize>>, LocatedBudgetError> {
    let Some(owner) = types.get(type_name) else {
        return Ok(BTreeMap::new());
    };
    let Some(check) = owner.check.as_ref() else {
        return Ok(BTreeMap::new());
    };
    let mut by_dimension: BTreeMap<DimensionName, Vec<usize>> = BTreeMap::new();
    let mut analyzer = DimensionCheckAnalyzer::new(types, type_name, &owner.module, check.span)
        .with_budget(budget);
    for (index, stmt) in check.stmts.iter().enumerate() {
        for dimension in analyzer.stmt_dimensions(stmt)? {
            by_dimension.entry(dimension).or_default().push(index);
        }
    }
    Ok(by_dimension)
}

struct DimensionCheckAnalyzer<'schema, 'budget> {
    types: &'schema BTreeMap<TypeName, CftType>,
    current_type: &'schema TypeName,
    scopes: Vec<BTreeSet<String>>,
    module: &'schema ModuleId,
    span: Span,
    budget: Option<&'budget mut StructuralBudget>,
}

impl<'schema, 'budget> DimensionCheckAnalyzer<'schema, 'budget> {
    fn new(
        types: &'schema BTreeMap<TypeName, CftType>,
        current_type: &'schema TypeName,
        module: &'schema ModuleId,
        span: Span,
    ) -> Self {
        Self {
            types,
            current_type,
            scopes: Vec::new(),
            module,
            span,
            budget: None,
        }
    }

    fn with_budget(mut self, budget: &'budget mut StructuralBudget) -> Self {
        self.budget = Some(budget);
        self
    }

    fn charge(&mut self) -> Result<(), LocatedBudgetError> {
        let Some(budget) = self.budget.as_deref_mut() else {
            return Ok(());
        };
        budget
            .charge_work(StructureKind::SchemaDependency, 1)
            .map_err(|error| LocatedBudgetError {
                error,
                module: self.module.clone(),
                span: self.span,
            })
    }

    fn stmt_dimensions(
        &mut self,
        stmt: &CftSchemaCheckStmt,
    ) -> Result<BTreeSet<DimensionName>, LocatedBudgetError> {
        self.charge()?;
        Ok(match stmt {
            CftSchemaCheckStmt::Expr(expr) => self.expr_dimensions(expr)?,
            CftSchemaCheckStmt::Quantifier {
                binding,
                collection,
                body,
                ..
            } => {
                let mut out = self.expr_dimensions(collection)?;
                self.scopes.push(BTreeSet::from([binding.clone()]));
                for stmt in body {
                    out.extend(self.stmt_dimensions(stmt)?);
                }
                let _ = self.scopes.pop();
                out
            }
            CftSchemaCheckStmt::When {
                condition, body, ..
            } => {
                let mut out = self.expr_dimensions(condition)?;
                for stmt in body {
                    out.extend(self.stmt_dimensions(stmt)?);
                }
                out
            }
        })
    }

    fn expr_dimensions(
        &mut self,
        expr: &CftSchemaCheckExpr,
    ) -> Result<BTreeSet<DimensionName>, LocatedBudgetError> {
        self.charge()?;
        Ok(match &expr.kind {
            CftSchemaCheckExprKind::Int(_)
            | CftSchemaCheckExprKind::Float(_)
            | CftSchemaCheckExprKind::Bool(_)
            | CftSchemaCheckExprKind::Null
            | CftSchemaCheckExprKind::String(_) => BTreeSet::new(),
            CftSchemaCheckExprKind::Name(name) => self.name_dimensions(name),
            CftSchemaCheckExprKind::Field { expr, .. }
            | CftSchemaCheckExprKind::Is { expr, .. }
            | CftSchemaCheckExprKind::Unary { expr, .. } => self.expr_dimensions(expr)?,
            CftSchemaCheckExprKind::Index { expr, index } => {
                let mut out = self.expr_dimensions(expr)?;
                out.extend(self.expr_dimensions(index)?);
                out
            }
            CftSchemaCheckExprKind::Call { args, .. } => self.args_dimensions(args)?,
            CftSchemaCheckExprKind::MethodCall { receiver, args, .. } => {
                let mut out = self.expr_dimensions(receiver)?;
                out.extend(self.args_dimensions(args)?);
                out
            }
            CftSchemaCheckExprKind::BinOp { lhs, rhs, .. } => {
                let mut out = self.expr_dimensions(lhs)?;
                out.extend(self.expr_dimensions(rhs)?);
                out
            }
            CftSchemaCheckExprKind::CmpChain { first, rest } => {
                let mut out = self.expr_dimensions(first)?;
                for (_, expr) in rest {
                    out.extend(self.expr_dimensions(expr)?);
                }
                out
            }
        })
    }

    fn name_dimensions(&self, name: &str) -> BTreeSet<DimensionName> {
        if self.scopes.iter().rev().any(|scope| scope.contains(name)) {
            return BTreeSet::new();
        }
        self.types
            .get(self.current_type)
            .and_then(|ty| ty.field(name))
            .and_then(|field| field.dimension.as_ref())
            .map(|binding| BTreeSet::from([binding.dimension.clone()]))
            .unwrap_or_default()
    }

    fn args_dimensions(
        &mut self,
        args: &[CftSchemaCheckExpr],
    ) -> Result<BTreeSet<DimensionName>, LocatedBudgetError> {
        let mut out = BTreeSet::new();
        for arg in args {
            out.extend(self.expr_dimensions(arg)?);
        }
        Ok(out)
    }
}
