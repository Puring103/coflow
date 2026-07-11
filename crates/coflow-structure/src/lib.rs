//! Domain-neutral limits for recursive structure and evaluator work.

#![cfg_attr(
    not(test),
    deny(
        clippy::dbg_macro,
        clippy::expect_used,
        clippy::panic,
        clippy::panic_in_result_fn,
        clippy::todo,
        clippy::unimplemented,
        clippy::unreachable,
        clippy::unwrap_used
    )
)]

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StructuralLimits {
    pub max_depth: u64,
    pub max_nodes: u64,
    pub max_work: u64,
}

impl StructuralLimits {
    #[must_use]
    pub const fn new(max_depth: u64, max_nodes: u64, max_work: u64) -> Self {
        Self {
            max_depth,
            max_nodes,
            max_work,
        }
    }
}

impl Default for StructuralLimits {
    fn default() -> Self {
        Self::new(256, 1_000_000, 10_000_000)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TraversalCursor {
    depth: u64,
}

impl TraversalCursor {
    #[must_use]
    pub const fn root() -> Self {
        Self { depth: 0 }
    }

    #[must_use]
    pub const fn depth(self) -> u64 {
        self.depth
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetAxis {
    Depth,
    Nodes,
    Work,
}

impl fmt::Display for BudgetAxis {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Depth => "depth",
            Self::Nodes => "nodes",
            Self::Work => "work",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructureKind {
    TypeRef,
    DefaultValue,
    CheckAst,
    SchemaDependency,
    DataValue,
    SpreadResolution,
    CheckEvaluation,
    QuantifierIteration,
}

impl fmt::Display for StructureKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::TypeRef => "type ref",
            Self::DefaultValue => "default value",
            Self::CheckAst => "check AST",
            Self::SchemaDependency => "schema dependency",
            Self::DataValue => "data value",
            Self::SpreadResolution => "spread resolution",
            Self::CheckEvaluation => "check evaluation",
            Self::QuantifierIteration => "quantifier iteration",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BudgetExceeded {
    pub axis: BudgetAxis,
    pub limit: u64,
    pub observed: u64,
    pub kind: StructureKind,
}

impl fmt::Display for BudgetExceeded {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} exceeds structural {} limit {} (observed {})",
            self.kind, self.axis, self.limit, self.observed
        )
    }
}

impl std::error::Error for BudgetExceeded {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructuralBudget {
    limits: StructuralLimits,
    nodes_used: u64,
    work_used: u64,
}

impl StructuralBudget {
    #[must_use]
    pub const fn new(limits: StructuralLimits) -> Self {
        Self {
            limits,
            nodes_used: 0,
            work_used: 0,
        }
    }

    #[must_use]
    pub const fn limits(&self) -> StructuralLimits {
        self.limits
    }

    #[must_use]
    pub const fn nodes_used(&self) -> u64 {
        self.nodes_used
    }

    #[must_use]
    pub const fn work_used(&self) -> u64 {
        self.work_used
    }

    pub fn enter(
        &mut self,
        cursor: TraversalCursor,
        kind: StructureKind,
        nodes: u64,
    ) -> Result<TraversalCursor, BudgetExceeded> {
        let observed_depth = cursor.depth.saturating_add(1);
        if observed_depth > self.limits.max_depth {
            return Err(BudgetExceeded {
                axis: BudgetAxis::Depth,
                limit: self.limits.max_depth,
                observed: observed_depth,
                kind,
            });
        }
        self.charge_nodes(kind, nodes)?;
        Ok(TraversalCursor {
            depth: observed_depth,
        })
    }

    pub fn charge_nodes(&mut self, kind: StructureKind, nodes: u64) -> Result<(), BudgetExceeded> {
        let observed = self.nodes_used.saturating_add(nodes);
        if observed > self.limits.max_nodes {
            return Err(BudgetExceeded {
                axis: BudgetAxis::Nodes,
                limit: self.limits.max_nodes,
                observed,
                kind,
            });
        }
        self.nodes_used = observed;
        Ok(())
    }

    pub fn charge_work(&mut self, kind: StructureKind, work: u64) -> Result<(), BudgetExceeded> {
        let observed = self.work_used.saturating_add(work);
        if observed > self.limits.max_work {
            return Err(BudgetExceeded {
                axis: BudgetAxis::Work,
                limit: self.limits.max_work,
                observed,
                kind,
            });
        }
        self.work_used = observed;
        Ok(())
    }
}

impl Default for StructuralBudget {
    fn default() -> Self {
        Self::new(StructuralLimits::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn depth_limit_accepts_boundary_and_rejects_first_child_beyond_it() {
        let mut budget = StructuralBudget::new(StructuralLimits::new(2, 10, 10));
        let one = budget
            .enter(TraversalCursor::root(), StructureKind::DataValue, 1)
            .expect("depth one");
        let two = budget
            .enter(one, StructureKind::DataValue, 1)
            .expect("depth two");

        assert_eq!(two.depth(), 2);
        assert_eq!(budget.nodes_used(), 2);
        assert_eq!(
            budget.enter(two, StructureKind::DataValue, 1),
            Err(BudgetExceeded {
                axis: BudgetAxis::Depth,
                limit: 2,
                observed: 3,
                kind: StructureKind::DataValue,
            })
        );
        assert_eq!(budget.nodes_used(), 2, "rejected nodes are not charged");
    }

    #[test]
    fn node_and_work_limits_have_stable_boundary_results() {
        let limits = StructuralLimits::new(10, 3, 4);
        let mut budget = StructuralBudget::new(limits);

        budget
            .charge_nodes(StructureKind::CheckAst, 3)
            .expect("node boundary");
        assert_eq!(
            budget.charge_nodes(StructureKind::CheckAst, 1),
            Err(BudgetExceeded {
                axis: BudgetAxis::Nodes,
                limit: 3,
                observed: 4,
                kind: StructureKind::CheckAst,
            })
        );
        budget
            .charge_work(StructureKind::CheckEvaluation, 4)
            .expect("work boundary");
        assert_eq!(
            budget.charge_work(StructureKind::QuantifierIteration, 1),
            Err(BudgetExceeded {
                axis: BudgetAxis::Work,
                limit: 4,
                observed: 5,
                kind: StructureKind::QuantifierIteration,
            })
        );
    }

    #[test]
    fn overflow_reports_saturated_observation_without_mutating_usage() {
        let mut budget = StructuralBudget::new(StructuralLimits::new(1, u64::MAX - 1, 0));
        budget
            .charge_nodes(StructureKind::SchemaDependency, u64::MAX - 1)
            .expect("initial charge");

        let error = budget
            .charge_nodes(StructureKind::SchemaDependency, 10)
            .expect_err("overflowing charge exceeds limit");
        assert_eq!(error.observed, u64::MAX);
        assert_eq!(budget.nodes_used(), u64::MAX - 1);
        assert_eq!(
            error.to_string(),
            "schema dependency exceeds structural nodes limit 18446744073709551614 (observed 18446744073709551615)"
        );
    }
}
