//! Checker-owned execution limits and budget accounting.

use std::fmt;

/// Public limits for one check task evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EvaluationLimits {
    pub max_work: u64,
    pub max_iterations: u64,
}

impl EvaluationLimits {
    #[must_use]
    pub const fn new(max_work: u64, max_iterations: u64) -> Self {
        Self {
            max_work,
            max_iterations,
        }
    }
}

impl Default for EvaluationLimits {
    fn default() -> Self {
        Self::new(10_000_000, 1_000_000)
    }
}

// 这些上限保护求值器内部结构，不属于可调的项目语义。
const MAX_EVALUATION_DEPTH: u64 = 256;
const MAX_TEMPORARY_NODES: u64 = 1_000_000;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct EvaluationCursor {
    depth: u64,
}

impl EvaluationCursor {
    #[must_use]
    pub(crate) const fn root() -> Self {
        Self { depth: 0 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EvaluationKind {
    Expression,
    DataValue,
}

impl fmt::Display for EvaluationKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Expression => "check evaluation",
            Self::DataValue => "data value",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EvaluationAxis {
    Depth,
    Nodes,
    Work,
    Iterations,
}

impl fmt::Display for EvaluationAxis {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Depth => "depth",
            Self::Nodes => "nodes",
            Self::Work => "work",
            Self::Iterations => "iterations",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EvaluationBudgetExceeded {
    axis: EvaluationAxis,
    limit: u64,
    observed: u64,
    kind: EvaluationKind,
}

impl fmt::Display for EvaluationBudgetExceeded {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} exceeds evaluation {} limit {} (observed {})",
            self.kind, self.axis, self.limit, self.observed
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EvaluationBudget {
    limits: EvaluationLimits,
    nodes_used: u64,
    work_used: u64,
    iterations_used: u64,
}

impl EvaluationBudget {
    #[must_use]
    pub(crate) const fn new(limits: EvaluationLimits) -> Self {
        Self {
            limits,
            nodes_used: 0,
            work_used: 0,
            iterations_used: 0,
        }
    }

    #[must_use]
    pub(crate) const fn work_used(&self) -> u64 {
        self.work_used
    }

    pub(crate) fn enter(
        &mut self,
        cursor: EvaluationCursor,
        kind: EvaluationKind,
        nodes: u64,
    ) -> Result<EvaluationCursor, EvaluationBudgetExceeded> {
        let observed = cursor.depth.saturating_add(1);
        if observed > MAX_EVALUATION_DEPTH {
            return Err(EvaluationBudgetExceeded {
                axis: EvaluationAxis::Depth,
                limit: MAX_EVALUATION_DEPTH,
                observed,
                kind,
            });
        }
        self.charge_nodes(kind, nodes)?;
        Ok(EvaluationCursor { depth: observed })
    }

    pub(crate) const fn charge_nodes(
        &mut self,
        kind: EvaluationKind,
        nodes: u64,
    ) -> Result<(), EvaluationBudgetExceeded> {
        let observed = self.nodes_used.saturating_add(nodes);
        if observed > MAX_TEMPORARY_NODES {
            return Err(EvaluationBudgetExceeded {
                axis: EvaluationAxis::Nodes,
                limit: MAX_TEMPORARY_NODES,
                observed,
                kind,
            });
        }
        self.nodes_used = observed;
        Ok(())
    }

    pub(crate) const fn charge_work(
        &mut self,
        kind: EvaluationKind,
        work: u64,
    ) -> Result<(), EvaluationBudgetExceeded> {
        let observed = self.work_used.saturating_add(work);
        if observed > self.limits.max_work {
            return Err(EvaluationBudgetExceeded {
                axis: EvaluationAxis::Work,
                limit: self.limits.max_work,
                observed,
                kind,
            });
        }
        self.work_used = observed;
        Ok(())
    }

    pub(crate) const fn charge_iterations(
        &mut self,
        iterations: u64,
    ) -> Result<(), EvaluationBudgetExceeded> {
        let observed = self.iterations_used.saturating_add(iterations);
        if observed > self.limits.max_iterations {
            return Err(EvaluationBudgetExceeded {
                axis: EvaluationAxis::Iterations,
                limit: self.limits.max_iterations,
                observed,
                kind: EvaluationKind::Expression,
            });
        }
        self.iterations_used = observed;
        Ok(())
    }
}

impl Default for EvaluationBudget {
    fn default() -> Self {
        Self::new(EvaluationLimits::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn work_and_iterations_have_independent_boundaries() {
        let mut budget = EvaluationBudget::new(EvaluationLimits::new(2, 1));
        budget
            .charge_work(EvaluationKind::Expression, 2)
            .expect("work boundary");
        budget.charge_iterations(1).expect("iteration boundary");
        assert_eq!(
            budget
                .charge_iterations(1)
                .expect_err("second iteration exceeds its own limit")
                .axis,
            EvaluationAxis::Iterations
        );
    }

    #[test]
    fn internal_depth_and_node_guards_keep_stable_boundaries() {
        let mut budget = EvaluationBudget::default();
        budget
            .enter(
                EvaluationCursor {
                    depth: MAX_EVALUATION_DEPTH - 1,
                },
                EvaluationKind::Expression,
                1,
            )
            .expect("depth boundary");
        assert_eq!(
            budget
                .enter(
                    EvaluationCursor {
                        depth: MAX_EVALUATION_DEPTH,
                    },
                    EvaluationKind::Expression,
                    1,
                )
                .expect_err("next depth is rejected")
                .axis,
            EvaluationAxis::Depth
        );

        budget.nodes_used = MAX_TEMPORARY_NODES;
        assert_eq!(
            budget
                .charge_nodes(EvaluationKind::DataValue, 1)
                .expect_err("next temporary node is rejected")
                .axis,
            EvaluationAxis::Nodes
        );
    }
}
