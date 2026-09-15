//! Checker-owned execution limits and budget accounting.

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
