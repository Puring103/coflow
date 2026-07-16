use coflow_data_model::CfdRecordId;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Default)]
pub struct DependencyGraph {
    pub reads_from: BTreeMap<CfdRecordId, BTreeSet<CfdRecordId>>,
}

impl DependencyGraph {
    #[must_use]
    pub fn affected_by(&self, changed: &[CfdRecordId]) -> Vec<CfdRecordId> {
        let mut out: BTreeSet<CfdRecordId> = changed.iter().copied().collect();
        let changed_set: BTreeSet<CfdRecordId> = changed.iter().copied().collect();
        for (reader, reads) in &self.reads_from {
            if reads.iter().any(|id| changed_set.contains(id)) {
                out.insert(*reader);
            }
        }
        out.into_iter().collect()
    }

    pub(crate) fn merge(&mut self, source: Self) {
        for (reader, reads) in source.reads_from {
            self.reads_from.entry(reader).or_default().extend(reads);
        }
    }
}

#[derive(Debug, Default)]
pub(super) struct DependencyCollector {
    enabled: bool,
    root_record: Option<CfdRecordId>,
    reads_from: BTreeSet<CfdRecordId>,
}

impl DependencyCollector {
    pub(super) fn disabled(root_record: Option<CfdRecordId>) -> Self {
        Self {
            enabled: false,
            root_record,
            reads_from: BTreeSet::new(),
        }
    }

    pub(super) fn enabled(root_record: Option<CfdRecordId>) -> Self {
        Self {
            enabled: true,
            root_record,
            reads_from: BTreeSet::new(),
        }
    }

    pub(super) fn note_read_from(&mut self, target: CfdRecordId) {
        if !self.enabled {
            return;
        }
        if self.root_record.is_some_and(|root| root == target) {
            return;
        }
        self.reads_from.insert(target);
    }

    pub(super) fn into_reads_from(self) -> BTreeSet<CfdRecordId> {
        self.reads_from
    }
}

#[derive(Debug, Default)]
pub(super) struct DependencyGraphBuilder {
    reads_from: BTreeMap<CfdRecordId, BTreeSet<CfdRecordId>>,
}

impl DependencyGraphBuilder {
    pub(super) fn new() -> Self {
        Self::default()
    }

    pub(super) fn collector_for(&self, root_record: Option<CfdRecordId>) -> DependencyCollector {
        DependencyCollector::enabled(root_record)
    }

    pub(super) fn extend_root(
        &mut self,
        root_record: Option<CfdRecordId>,
        collector: DependencyCollector,
    ) {
        let Some(root_record) = root_record else {
            return;
        };
        let reads_from = collector.into_reads_from();
        if reads_from.is_empty() {
            return;
        }
        self.reads_from
            .entry(root_record)
            .or_default()
            .extend(reads_from);
    }

    pub(super) fn finish(self) -> DependencyGraph {
        DependencyGraph {
            reads_from: self.reads_from,
        }
    }
}
