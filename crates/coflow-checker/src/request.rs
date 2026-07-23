use std::collections::{BTreeMap, BTreeSet};

use coflow_data_model::{CfdPath, CfdRecordId, RecordCoordinate};
use coflow_cft::{CftSchema, TypeName};
use coflow_structure::StructuralLimits;

use crate::{CheckSnapshot, DimensionCheckRound};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckTargets<'a> {
    All,
    Records(&'a [CfdRecordId]),
    Incremental {
        previous: &'a CheckSnapshot,
        changed: &'a CheckChangeSet,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangedPaths {
    All,
    Paths(BTreeSet<CfdPath>),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CheckChangeSet {
    pub records: BTreeMap<RecordCoordinate, ChangedPaths>,
    pub memberships: BTreeSet<TypeName>,
}

impl CheckChangeSet {
    #[must_use]
    pub fn from_records(
        schema: &CftSchema,
        records: impl IntoIterator<Item = RecordCoordinate>,
    ) -> Self {
        let mut changes = Self::default();
        for record in records {
            let actual_type = record.actual_type.clone();
            changes.memberships.insert(actual_type.clone());
            if let Some(ancestors) = schema.ancestor_type_names(&actual_type) {
                changes.memberships.extend(ancestors.iter().cloned());
            }
            changes.records.insert(record, ChangedPaths::All);
        }
        changes
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DependencyCollection {
    #[default]
    None,
    Reads,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckRequest<'a> {
    pub targets: CheckTargets<'a>,
    pub rounds: Vec<DimensionCheckRound>,
    pub structural_limits: StructuralLimits,
    pub dependency_collection: DependencyCollection,
}

impl CheckRequest<'static> {
    #[must_use]
    pub fn all() -> Self {
        Self {
            targets: CheckTargets::All,
            rounds: Vec::new(),
            structural_limits: StructuralLimits::default(),
            dependency_collection: DependencyCollection::None,
        }
    }
}

impl<'a> CheckRequest<'a> {
    #[must_use]
    pub fn records(targets: &'a [CfdRecordId]) -> Self {
        Self {
            targets: CheckTargets::Records(targets),
            rounds: Vec::new(),
            structural_limits: StructuralLimits::default(),
            dependency_collection: DependencyCollection::None,
        }
    }

    #[must_use]
    pub fn incremental(
        previous: &'a CheckSnapshot,
        changed: &'a CheckChangeSet,
    ) -> Self {
        Self {
            targets: CheckTargets::Incremental { previous, changed },
            rounds: Vec::new(),
            structural_limits: StructuralLimits::default(),
            dependency_collection: DependencyCollection::Reads,
        }
    }

    #[must_use]
    pub fn with_rounds(mut self, rounds: impl IntoIterator<Item = DimensionCheckRound>) -> Self {
        self.rounds = rounds.into_iter().collect();
        self
    }

    #[must_use]
    pub const fn with_structural_limits(mut self, structural_limits: StructuralLimits) -> Self {
        self.structural_limits = structural_limits;
        self
    }

    #[must_use]
    pub const fn with_dependency_collection(
        mut self,
        dependency_collection: DependencyCollection,
    ) -> Self {
        self.dependency_collection = dependency_collection;
        self
    }
}
