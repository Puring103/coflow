use coflow_model::CfdRecordId;
use coflow_language::cft::{CftSchema, CheckStatementId, DimensionName, VariantName};
use std::cmp::Ordering;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct CheckTask {
    pub target: CheckTarget,
    pub statement: CheckStatementId,
    pub projection: CheckProjection,
}

impl CheckTask {
    #[must_use]
    pub fn execution_cmp(&self, other: &Self, schema: &CftSchema) -> Ordering {
        self.target
            .cmp(&other.target)
            .then_with(|| self.statement.cmp(&other.statement))
            .then_with(|| projection_cmp(&self.projection, &other.projection, schema))
    }
}

fn projection_cmp(left: &CheckProjection, right: &CheckProjection, schema: &CftSchema) -> Ordering {
    match (left, right) {
        (CheckProjection::Base, CheckProjection::Base) => Ordering::Equal,
        (CheckProjection::Base, CheckProjection::Dimension { .. }) => Ordering::Less,
        (CheckProjection::Dimension { .. }, CheckProjection::Base) => Ordering::Greater,
        (
            CheckProjection::Dimension {
                dimension: left_dimension,
                variant: left_variant,
            },
            CheckProjection::Dimension {
                dimension: right_dimension,
                variant: right_variant,
            },
        ) => left_dimension.cmp(right_dimension).then_with(|| {
            let meta = schema.resolve_dimension(left_dimension);
            let left_index = meta
                .and_then(|dimension| dimension.variant_index(left_variant))
                .unwrap_or(usize::MAX);
            let right_index = meta
                .and_then(|dimension| dimension.variant_index(right_variant))
                .unwrap_or(usize::MAX);
            left_index
                .cmp(&right_index)
                .then_with(|| left_variant.cmp(right_variant))
        }),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CheckTarget {
    Record(CfdRecordId),
    Project,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum CheckProjection {
    Base,
    Dimension {
        dimension: DimensionName,
        variant: VariantName,
    },
}

impl CheckProjection {
    #[must_use]
    pub const fn dimension(&self) -> Option<(&DimensionName, &VariantName)> {
        match self {
            Self::Base => None,
            Self::Dimension { dimension, variant } => Some((dimension, variant)),
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
