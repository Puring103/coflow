use crate::module::ModuleId;
use crate::schema::check_visit::CheckVisitor;
use crate::schema::LocatedBudgetError;
use crate::{
    CftSchemaCheckExpr, CftSchemaCheckStmt, CftSchemaQuantifierBindings, CftType, DimensionName,
    Span, TypeName,
};
use coflow_structure::{StructuralBudget, StructureKind};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Default)]
pub(super) struct CheckDependencyPlan {
    dimension_statements: BTreeMap<DimensionName, Vec<usize>>,
}

impl CheckDependencyPlan {
    pub(super) fn statement_indices(&self, dimension: &str) -> Option<&[usize]> {
        self.dimension_statements.get(dimension).map(Vec::as_slice)
    }
}

pub(super) fn check_dependencies_for_type(
    types: &BTreeMap<TypeName, CftType>,
    type_name: &TypeName,
    budget: &mut StructuralBudget,
) -> Result<CheckDependencyPlan, LocatedBudgetError> {
    let Some(owner) = types.get(type_name) else {
        return Ok(CheckDependencyPlan::default());
    };
    let Some(check) = owner.check.as_ref() else {
        return Ok(CheckDependencyPlan::default());
    };
    let mut by_dimension: BTreeMap<DimensionName, Vec<usize>> = BTreeMap::new();
    let mut analyzer = CheckDependencyAnalyzer::new(types, type_name, &owner.module, check.span, budget);
    for (index, stmt) in check.stmts.iter().enumerate() {
        analyzer.dimensions.clear();
        analyzer.visit_stmt(stmt)?;
        for dimension in &analyzer.dimensions {
            by_dimension.entry(dimension.clone()).or_default().push(index);
        }
    }
    Ok(CheckDependencyPlan {
        dimension_statements: by_dimension,
    })
}

struct CheckDependencyAnalyzer<'schema, 'budget> {
    types: &'schema BTreeMap<TypeName, CftType>,
    current_type: &'schema TypeName,
    scopes: Vec<BTreeSet<String>>,
    dimensions: BTreeSet<DimensionName>,
    module: &'schema ModuleId,
    span: Span,
    budget: &'budget mut StructuralBudget,
}

impl<'schema, 'budget> CheckDependencyAnalyzer<'schema, 'budget> {
    fn new(
        types: &'schema BTreeMap<TypeName, CftType>,
        current_type: &'schema TypeName,
        module: &'schema ModuleId,
        span: Span,
        budget: &'budget mut StructuralBudget,
    ) -> Self {
        Self {
            types,
            current_type,
            scopes: Vec::new(),
            dimensions: BTreeSet::new(),
            module,
            span,
            budget,
        }
    }

    fn charge(&mut self) -> Result<(), LocatedBudgetError> {
        self.budget
            .charge_work(StructureKind::SchemaDependency, 1)
            .map_err(|error| LocatedBudgetError {
                error,
                module: self.module.clone(),
                span: self.span,
            })
    }
}

impl CheckVisitor for CheckDependencyAnalyzer<'_, '_> {
    type Error = LocatedBudgetError;

    fn visit_stmt(&mut self, stmt: &CftSchemaCheckStmt) -> Result<(), Self::Error> {
        self.charge()?;
        self.walk_stmt(stmt)
    }

    fn visit_expr(&mut self, expr: &CftSchemaCheckExpr) -> Result<(), Self::Error> {
        self.charge()?;
        self.walk_expr(expr)
    }

    fn enter_quantifier_body(
        &mut self,
        bindings: &CftSchemaQuantifierBindings,
    ) -> Result<(), Self::Error> {
        let names = match bindings {
            CftSchemaQuantifierBindings::Single { binding } => {
                BTreeSet::from([binding.clone()])
            }
            CftSchemaQuantifierBindings::Array { item, index } => {
                BTreeSet::from([item.clone(), index.clone()])
            }
            CftSchemaQuantifierBindings::Dict { key, value } => {
                BTreeSet::from([key.clone(), value.clone()])
            }
        };
        self.scopes.push(names);
        Ok(())
    }

    fn exit_quantifier_body(
        &mut self,
        _bindings: &CftSchemaQuantifierBindings,
    ) -> Result<(), Self::Error> {
        let _ = self.scopes.pop();
        Ok(())
    }

    fn visit_name(&mut self, name: &str) -> Result<(), Self::Error> {
        if !self.scopes.iter().rev().any(|scope| scope.contains(name)) {
            if let Some(dimension) = self
                .types
                .get(self.current_type)
                .and_then(|ty| ty.field(name))
                .and_then(|field| field.dimension.as_ref())
                .map(|binding| binding.dimension.clone())
            {
                self.dimensions.insert(dimension);
            }
        }
        Ok(())
    }
}
