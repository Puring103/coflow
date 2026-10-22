use coflow_language::cft::{CftSchema, CheckStatementId};
use coflow_model::{CfdPath, CfdRecordHandle};
use std::cmp::Ordering;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct CheckTask {
    pub target: CheckTarget,
    pub statement: CheckStatementId,
}

impl CheckTask {
    #[must_use]
    pub fn execution_cmp(&self, other: &Self, schema: &CftSchema) -> Ordering {
        self.target.cmp(&other.target).then_with(|| {
            let left = schema.check_statement(self.statement);
            let right = schema.check_statement(other.statement);
            if let (Some(left), Some(right)) = (left, right) {
                if let (
                    coflow_language::cft::CheckOwner::Type(a),
                    coflow_language::cft::CheckOwner::Type(b),
                ) = (&left.info.owner, &right.info.owner)
                {
                    if a != b {
                        if schema.check_owner_applies_to_actual(a, b) {
                            return Ordering::Less;
                        }
                        if schema.check_owner_applies_to_actual(b, a) {
                            return Ordering::Greater;
                        }
                    }
                }
            }
            self.statement.cmp(&other.statement)
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum CheckTarget {
    Record(CfdRecordHandle),
    Data {
        record: CfdRecordHandle,
        path: CfdPath,
        dimension: Option<(String, String)>,
    },
    Project,
}

impl CheckTarget {
    #[must_use]
    pub const fn record(&self) -> Option<CfdRecordHandle> {
        match self {
            Self::Record(record) | Self::Data { record, .. } => Some(*record),
            Self::Project => None,
        }
    }
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
