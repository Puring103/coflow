use super::SymbolTable;
use crate::limits::{StructuralBudget, StructureKind, TraversalCursor};
use crate::schema::LocatedBudgetError;
use crate::source::Span;
use coflow_language::cft::syntax::ast::{
    Annotation, DefaultExpr, DefaultExprKind, Item, TypeRef, TypeRefKind,
};
use coflow_language::cft::ModuleId;
use coflow_language::diagnostics::{CftDiagnostic, CftErrorCode};

impl SymbolTable<'_> {
    pub(super) fn validate_structure(&mut self, budget: &mut StructuralBudget) -> bool {
        let modules = self.modules;
        for (module_id, module) in modules.modules() {
            let Some(ast) = module.ast() else {
                continue;
            };
            if let Err(error) =
                validate_module(budget, module_id, &ast.items, &ast.dangling_annotations)
            {
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
                    walk_value_type(budget, module, ty)?;
                }
                walk_default(budget, module, &definition.value)?;
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
                    walk_value_type(budget, module, &field.ty)?;
                    if let Some(default) = &field.default {
                        walk_default(budget, module, default)?;
                    }
                }
                if let Some(check) = &definition.check {
                    charge_flat(budget, module, check.span, 1)?;
                }
            }
            Item::TypeAlias(definition) => {
                charge_annotations(budget, module, &definition.annotations)?;
                walk_value_type(budget, module, &definition.target)?;
            }
            Item::Check(definition) => {
                charge_annotations(budget, module, &definition.annotations)?;
                charge_flat(budget, module, definition.block.span, 1)?;
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
        .map_err(|error| LocatedBudgetError {
            error,
            module: module.clone(),
            span,
        })
}

fn walk_value_type(
    budget: &mut StructuralBudget,
    module: &ModuleId,
    root: &TypeRef,
) -> Result<(), LocatedBudgetError> {
    let mut pending = vec![(root, TraversalCursor::root())];
    while let Some((ty, parent)) = pending.pop() {
        let cursor = enter(budget, module, ty.span, parent, StructureKind::TypeRef)?;
        match &ty.kind {
            TypeRefKind::Array(inner) | TypeRefKind::Option(inner) => {
                pending.push((inner, cursor));
            }
            TypeRefKind::Dict(key, value) => {
                pending.push((value, cursor));
                pending.push((key, cursor));
            }
            TypeRefKind::Function(parameters, result) => {
                pending.push((result, cursor));
                pending.extend(
                    parameters
                        .iter()
                        .rev()
                        .map(|parameter| (&parameter.value_type, cursor)),
                );
            }
            TypeRefKind::Int
            | TypeRefKind::Float
            | TypeRefKind::Bool
            | TypeRefKind::String
            | TypeRefKind::FString
            | TypeRefKind::Unit
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
            DefaultExprKind::TypedObject { fields, .. } => {
                pending.extend(fields.iter().rev().map(|(_, value)| (value, cursor)));
            }
            DefaultExprKind::BitExpr { lhs, rhs, .. } => {
                pending.push((rhs, cursor));
                pending.push((lhs, cursor));
            }
            DefaultExprKind::Dictionary(entries) => {
                for (key, value) in entries.iter().rev() {
                    pending.push((value, cursor));
                    pending.push((key, cursor));
                }
            }
            DefaultExprKind::Int(_)
            | DefaultExprKind::Float(_)
            | DefaultExprKind::Bool(_)
            | DefaultExprKind::OptionNone
            | DefaultExprKind::String(_)
            | DefaultExprKind::FormattedString(_)
            | DefaultExprKind::Function { .. }
            | DefaultExprKind::StaticPath(_)
            | DefaultExprKind::RecordReference(_) => {}
        }
    }
    Ok(())
}
