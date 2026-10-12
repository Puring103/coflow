use coflow_language::cft::{CftSchema, CheckStatementId};
use coflow_model::CfdRecordId;
use std::cmp::Ordering;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct CheckTask {
    pub target: CheckTarget,
    pub statement: CheckStatementId,
}

impl CheckTask {
    #[must_use]
    pub fn execution_cmp(&self, other: &Self, _schema: &CftSchema) -> Ordering {
        self.target
            .cmp(&other.target)
            .then_with(|| self.statement.cmp(&other.statement))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CheckTarget {
    Record(CfdRecordId),
    Project,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CheckLimits {
    pub evaluation: crate::EvaluationLimits,
    pub max_tasks: usize,
    pub max_request_work: u64,
}

impl Default for CheckLimits {
    fn default() -> Self {
        Self {
            evaluation: crate::EvaluationLimits::default(),
            max_tasks: 1_000_000,
            max_request_work: 100_000_000,
        }
    }
}
