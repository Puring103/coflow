use super::SchemaCompiler;
use crate::ast::{
    Annotation, CheckBlock, CheckExpr, CheckExprKind, CheckStmt, DefaultExpr, DefaultExprKind,
    Item, TypeRef, TypeRefKind,
};
use crate::container::ModuleId;
use crate::error::{CftDiagnostic, CftErrorCode};
use crate::span::Span;
use coflow_structure::{BudgetExceeded, StructuralBudget, StructureKind, TraversalCursor};

struct LocatedBudgetError {
    error: BudgetExceeded,
    module: ModuleId,
    span: Span,
}

impl SchemaCompiler<'_> {
    pub(super) fn validate_structure(&mut self) -> bool {
        let modules = self.modules;
        let budget = &mut self.budget;
        for (module_id, module) in &modules.modules {
            if let Err(error) = validate_module(
                budget,
                module_id,
                &module.ast.items,
                &module.ast.dangling_annotations,
            ) {
                self.diagnostics.push(CftDiagnostic::error(
                    CftErrorCode::SchemaStructureLimitExceeded,
                    error.module,
                    error.span,
                    error.error.to_string(),
                ));
                return false;
            }
        }
        true
    }

    pub(super) fn push_budget_error(
        &mut self,
        error: BudgetExceeded,
        module: &ModuleId,
        span: Span,
    ) {
        self.diagnostics.push(CftDiagnostic::error(
            CftErrorCode::SchemaStructureLimitExceeded,
            module.clone(),
            span,
            error.to_string(),
        ));
    }
}

fn validate_module(
    budget: &mut StructuralBudget,
    module: &ModuleId,
    items: &[Item],
    dangling_annotations: &[Annotation],
) -> Result<(), LocatedBudgetError> {
    for annotation in dangling_annotations {
        charge_annotation(budget, module, annotation)?;
    }
    for item in items {
        charge_flat(budget, module, item.span(), 1)?;
        match item {
            Item::Const(definition) => {
                charge_annotations(budget, module, &definition.annotations)?;
                if let Some(ty) = &definition.ty {
                    walk_type_ref(budget, module, ty)?;
                }
            }
            Item::Enum(definition) => {
                charge_annotations(budget, module, &definition.annotations)?;
                charge_annotations(budget, module, &definition.dangling_annotations)?;
                for variant in &definition.variants {
                    charge_flat(budget, module, variant.span, 1)?;
                    charge_annotations(budget, module, &variant.annotations)?;
                }
            }
            Item::Type(definition) => {
                charge_annotations(budget, module, &definition.annotations)?;
                charge_annotations(budget, module, &definition.dangling_annotations)?;
                for field in &definition.fields {
                    charge_flat(budget, module, field.span, 1)?;
                    charge_annotations(budget, module, &field.annotations)?;
                    walk_type_ref(budget, module, &field.ty)?;
                    if let Some(default) = &field.default {
                        walk_default(budget, module, default)?;
                    }
                }
                if let Some(check) = &definition.check {
                    walk_check(budget, module, check)?;
                }
            }
        }
    }
    Ok(())
}

fn charge_annotations(
    budget: &mut StructuralBudget,
    module: &ModuleId,
    annotations: &[Annotation],
) -> Result<(), LocatedBudgetError> {
    for annotation in annotations {
        charge_annotation(budget, module, annotation)?;
    }
    Ok(())
}

fn charge_annotation(
    budget: &mut StructuralBudget,
    module: &ModuleId,
    annotation: &Annotation,
) -> Result<(), LocatedBudgetError> {
    let nodes = u64::try_from(annotation.args.len())
        .unwrap_or(u64::MAX)
        .saturating_add(1);
    charge_flat(budget, module, annotation.span, nodes)
}

fn charge_flat(
    budget: &mut StructuralBudget,
    module: &ModuleId,
    span: Span,
    nodes: u64,
) -> Result<(), LocatedBudgetError> {
    budget
        .charge_nodes(StructureKind::SchemaAst, nodes)
        .and_then(|()| budget.charge_work(StructureKind::SchemaAst, nodes))
        .map_err(|error| LocatedBudgetError {
            error,
            module: module.clone(),
            span,
        })
}

fn enter(
    budget: &mut StructuralBudget,
    module: &ModuleId,
    span: Span,
    parent: TraversalCursor,
    kind: StructureKind,
) -> Result<TraversalCursor, LocatedBudgetError> {
    budget
        .enter(parent, kind, 1)
        .and_then(|cursor| budget.charge_work(kind, 1).map(|()| cursor))
        .map_err(|error| LocatedBudgetError {
            error,
            module: module.clone(),
            span,
        })
}

fn walk_type_ref(
    budget: &mut StructuralBudget,
    module: &ModuleId,
    root: &TypeRef,
) -> Result<(), LocatedBudgetError> {
    let mut pending = vec![(root, TraversalCursor::root())];
    while let Some((ty, parent)) = pending.pop() {
        let cursor = enter(budget, module, ty.span, parent, StructureKind::TypeRef)?;
        match &ty.kind {
            TypeRefKind::Ref(inner) | TypeRefKind::Array(inner) | TypeRefKind::Nullable(inner) => {
                pending.push((inner, cursor));
            }
            TypeRefKind::Dict(key, value) => {
                pending.push((value, cursor));
                pending.push((key, cursor));
            }
            TypeRefKind::Int
            | TypeRefKind::Float
            | TypeRefKind::Bool
            | TypeRefKind::String
            | TypeRefKind::Named(_) => {}
        }
    }
    Ok(())
}

fn walk_default(
    budget: &mut StructuralBudget,
    module: &ModuleId,
    root: &DefaultExpr,
) -> Result<(), LocatedBudgetError> {
    let mut pending = vec![(root, TraversalCursor::root())];
    while let Some((value, parent)) = pending.pop() {
        let cursor = enter(
            budget,
            module,
            value.span,
            parent,
            StructureKind::DefaultValue,
        )?;
        match &value.kind {
            DefaultExprKind::Array(items) => {
                pending.extend(items.iter().rev().map(|item| (item, cursor)));
            }
            DefaultExprKind::Object(fields) => {
                pending.extend(fields.iter().rev().map(|(_, value)| (value, cursor)));
            }
            DefaultExprKind::Int(_)
            | DefaultExprKind::Float(_)
            | DefaultExprKind::Bool(_)
            | DefaultExprKind::Null
            | DefaultExprKind::String(_)
            | DefaultExprKind::Name(_)
            | DefaultExprKind::EnumVariant { .. } => {}
        }
    }
    Ok(())
}

enum CheckNode<'a> {
    Stmt(&'a CheckStmt),
    Expr(&'a CheckExpr),
}

fn walk_check(
    budget: &mut StructuralBudget,
    module: &ModuleId,
    check: &CheckBlock,
) -> Result<(), LocatedBudgetError> {
    let root = enter(
        budget,
        module,
        check.span,
        TraversalCursor::root(),
        StructureKind::CheckAst,
    )?;
    let mut pending = check
        .stmts
        .iter()
        .rev()
        .map(|stmt| (CheckNode::Stmt(stmt), root))
        .collect::<Vec<_>>();
    while let Some((node, parent)) = pending.pop() {
        let (span, children) = match node {
            CheckNode::Stmt(stmt) => {
                let children = match stmt {
                    CheckStmt::Expr(expr) => vec![CheckNode::Expr(expr)],
                    CheckStmt::When {
                        condition, body, ..
                    } => std::iter::once(CheckNode::Expr(condition))
                        .chain(body.iter().map(CheckNode::Stmt))
                        .collect(),
                    CheckStmt::Quantifier {
                        collection, body, ..
                    } => std::iter::once(CheckNode::Expr(collection))
                        .chain(body.iter().map(CheckNode::Stmt))
                        .collect(),
                };
                (stmt.span(), children)
            }
            CheckNode::Expr(expr) => (expr.span, check_expr_children(expr)),
        };
        let cursor = enter(budget, module, span, parent, StructureKind::CheckAst)?;
        pending.extend(children.into_iter().rev().map(|child| (child, cursor)));
    }
    Ok(())
}

fn check_expr_children(expr: &CheckExpr) -> Vec<CheckNode<'_>> {
    match &expr.kind {
        CheckExprKind::Field { expr, .. }
        | CheckExprKind::Is { expr, .. }
        | CheckExprKind::Unary { expr, .. } => vec![CheckNode::Expr(expr)],
        CheckExprKind::Index { expr, index } => {
            vec![CheckNode::Expr(expr), CheckNode::Expr(index)]
        }
        CheckExprKind::BinOp { lhs, rhs, .. } => {
            vec![CheckNode::Expr(lhs), CheckNode::Expr(rhs)]
        }
        CheckExprKind::CmpChain { first, rest } => std::iter::once(CheckNode::Expr(first))
            .chain(rest.iter().map(|(_, expr)| CheckNode::Expr(expr)))
            .collect(),
        CheckExprKind::Call { args, .. } => args.iter().map(CheckNode::Expr).collect(),
        CheckExprKind::MethodCall { receiver, args, .. } => {
            std::iter::once(CheckNode::Expr(receiver))
                .chain(args.iter().map(CheckNode::Expr))
                .collect()
        }
        CheckExprKind::Int(_)
        | CheckExprKind::Float(_)
        | CheckExprKind::Bool(_)
        | CheckExprKind::Null
        | CheckExprKind::String(_)
        | CheckExprKind::Name(_) => Vec::new(),
    }
}
