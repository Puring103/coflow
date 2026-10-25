use coflow_language::cft::{CftSchema, CheckStatementId};
use coflow_model::{CfdPath, CfdRecordHandle};
use std::cmp::Ordering;
use std::collections::BTreeSet;

/// A unique, ordered check plan bound to the schema that established its order.
#[derive(Debug)]
pub struct CheckPlan<'schema> {
    pub(crate) schema: &'schema CftSchema,
    pub(crate) tasks: Vec<CheckTask>,
}

impl<'schema> CheckPlan<'schema> {
    /// Builds an execution plan from tasks whose uniqueness is guaranteed by the set.
    #[must_use]
    pub fn from_unique_tasks(schema: &'schema CftSchema, tasks: BTreeSet<CheckTask>) -> Self {
        let mut tasks = tasks.into_iter().collect::<Vec<_>>();
        // 计划绑定 schema，并且只排序一次；执行入口仍检查本次请求的任务预算。
        tasks.sort_by(|left, right| left.execution_cmp(right, schema));
        Self { schema, tasks }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.tasks.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }

    #[must_use]
    pub fn into_tasks(self) -> Vec<CheckTask> {
        self.tasks
    }

    /// Executes this plan while preserving per-root and per-request budgets.
    #[must_use]
    pub fn execute(
        self,
        model: &coflow_model::CfdDataModel,
        limits: CheckLimits,
    ) -> crate::CheckOutput {
        crate::engine::execute_plan(self, model, limits)
    }
}

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
