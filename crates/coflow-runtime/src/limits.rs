//! Runtime composition of language/model structure limits and checker limits.

use coflow_checker::EvaluationLimits;
use coflow_language::limits::StructuralLimits;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct RuntimeLimits {
    pub(crate) structural: StructuralLimits,
    pub(crate) evaluation: EvaluationLimits,
}
