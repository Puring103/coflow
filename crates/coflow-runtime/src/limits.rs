//! Runtime composition of language/model structure limits and checker limits.

use coflow_checker::EvaluationLimits;
use coflow_language::limits::StructuralLimits;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RuntimeLimits {
    pub(crate) structural: StructuralLimits,
    pub(crate) evaluation: EvaluationLimits,
}

impl Default for RuntimeLimits {
    fn default() -> Self {
        Self {
            structural: StructuralLimits::default(),
            evaluation: EvaluationLimits::default(),
        }
    }
}
