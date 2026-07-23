use coflow_data_model::{CfdPath, CfdRecordId};
use crate::CheckExecutionId;
use coflow_cft::TypeName;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Default)]
pub struct DependencyGraph {
    pub reads_from: BTreeMap<CheckExecutionId, BTreeSet<RecordReadDependency>>,
    pub record_sets: BTreeMap<CheckExecutionId, BTreeSet<TypeName>>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct RecordReadDependency {
    pub record: CfdRecordId,
    pub path: CfdPath,
}

impl DependencyGraph {
    pub(crate) fn merge(&mut self, source: Self) {
        for (reader, reads) in source.reads_from {
            self.reads_from.entry(reader).or_default().extend(reads);
        }
        for (reader, record_sets) in source.record_sets {
            self.record_sets.entry(reader).or_default().extend(record_sets);
        }
    }
}

#[derive(Debug)]
pub(super) struct DependencyCollector {
    enabled: bool,
    root: Option<CheckExecutionId>,
    reads_from: BTreeSet<RecordReadDependency>,
}

impl DependencyCollector {
    pub(super) fn disabled(root: Option<CheckExecutionId>) -> Self {
        Self {
            enabled: false,
            root,
            reads_from: BTreeSet::new(),
        }
    }

    pub(super) fn enabled(root: CheckExecutionId) -> Self {
        Self {
            enabled: true,
            root: Some(root),
            reads_from: BTreeSet::new(),
        }
    }

    pub(super) fn note_read_from(&mut self, target: CfdRecordId, path: CfdPath) {
        if !self.enabled {
            return;
        }
        if matches!(self.root, Some(CheckExecutionId::Record(root)) if root == target) {
            return;
        }
        self.reads_from.insert(RecordReadDependency { record: target, path });
    }

    pub(super) fn into_reads_from(self) -> BTreeSet<RecordReadDependency> {
        self.reads_from
    }
}

#[derive(Debug, Default)]
pub(super) struct DependencyGraphBuilder {
    reads_from: BTreeMap<CheckExecutionId, BTreeSet<RecordReadDependency>>,
}

impl DependencyGraphBuilder {
    pub(super) fn new() -> Self {
        Self::default()
    }

    pub(super) fn collector_for(root: CheckExecutionId) -> DependencyCollector {
        DependencyCollector::enabled(root)
    }

    pub(super) fn extend_root(
        &mut self,
        root: CheckExecutionId,
        collector: DependencyCollector,
    ) {
        let reads_from = collector.into_reads_from();
        if reads_from.is_empty() {
            return;
        }
        self.reads_from
            .entry(root)
            .or_default()
            .extend(reads_from);
    }

    pub(super) fn finish(self) -> DependencyGraph {
        DependencyGraph {
            reads_from: self.reads_from,
            record_sets: BTreeMap::new(),
        }
    }
}
