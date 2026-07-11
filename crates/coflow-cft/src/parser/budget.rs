use super::Parser;
use crate::error::{CftDiagnostic, CftDiagnostics, CftErrorCode};
use crate::span::Span;
use coflow_structure::{BudgetExceeded, StructuralLimits, StructureKind, TraversalCursor};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CftParseOptions {
    pub structural_limits: StructuralLimits,
}

#[derive(Debug)]
pub(crate) struct Parsed<T> {
    pub(super) value: T,
    pub(super) depth: u64,
}

impl Parser<'_> {
    pub(super) fn node<T>(
        &mut self,
        kind: StructureKind,
        span: Span,
        child_depths: impl IntoIterator<Item = u64>,
        build: impl FnOnce() -> T,
    ) -> Result<Parsed<T>, CftDiagnostics> {
        let depth = child_depths
            .into_iter()
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        self.map_budget(
            self.budget
                .check_additional_depth(TraversalCursor::root(), kind, depth),
            span,
        )?;
        let node_charge = self.budget.charge_nodes(kind, 1);
        self.map_budget(node_charge, span)?;
        let work_charge = self.budget.charge_work(kind, 1);
        self.map_budget(work_charge, span)?;
        Ok(Parsed {
            value: build(),
            depth,
        })
    }

    pub(super) fn charge_nodes(
        &mut self,
        kind: StructureKind,
        span: Span,
        nodes: u64,
    ) -> Result<(), CftDiagnostics> {
        let node_charge = self.budget.charge_nodes(kind, nodes);
        self.map_budget(node_charge, span)?;
        let work_charge = self.budget.charge_work(kind, nodes);
        self.map_budget(work_charge, span)
    }

    pub(super) fn nested<T>(
        &mut self,
        kind: StructureKind,
        span: Span,
        parse: impl FnOnce(&mut Self) -> Result<T, CftDiagnostics>,
    ) -> Result<T, CftDiagnostics> {
        let observed = self.open_nesting.saturating_add(1);
        self.map_budget(
            self.budget
                .check_additional_depth(TraversalCursor::root(), kind, observed),
            span,
        )?;
        self.open_nesting = observed;
        let result = parse(self);
        self.open_nesting = self.open_nesting.saturating_sub(1);
        result
    }

    fn map_budget<T>(
        &self,
        result: Result<T, BudgetExceeded>,
        span: Span,
    ) -> Result<T, CftDiagnostics> {
        result.map_err(|error| {
            CftDiagnostics::one(CftDiagnostic::error(
                CftErrorCode::SyntaxStructureLimitExceeded,
                self.module.clone(),
                span,
                error.to_string(),
            ))
        })
    }
}
