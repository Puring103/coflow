/// Reason an incremental generation path had to execute full-scope work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IncrementalFallbackReason {
    SchemaChanged,
    RecordInserted,
    RecordDeleted,
    RecordRenamed,
    RecordReordered,
    SourceTopologyChanged,
    DimensionConfigurationChanged,
    ProviderConfigurationChanged,
    UnstableCoordinateMapping,
    IncompleteDependencyState,
}

/// Deterministic work counters for the latest immutable project generation.
///
/// These counters describe actual runtime work. They are deliberately kept
/// outside editor and mutation wire DTOs so observability does not alter
/// serialized compatibility contracts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProjectExecutionStats {
    pub sources_resolved: usize,
    pub sources_reloaded: usize,
    pub draft_records_collected: usize,
    pub records_validated: usize,
    pub records_materialized: usize,
    pub records_reused: usize,
    pub ref_edges_rebuilt: usize,
    pub spread_edges_rebuilt: usize,
    pub check_roots_executed: usize,
    pub dimension_records_projected: usize,
    pub dimension_sources_planned: usize,
    pub dimension_sources_written: usize,
    pub full_fallback: bool,
    pub fallback_reason: Option<IncrementalFallbackReason>,
}

impl ProjectExecutionStats {
    pub(crate) fn merge(&mut self, other: Self) {
        self.sources_resolved = self.sources_resolved.saturating_add(other.sources_resolved);
        self.sources_reloaded = self.sources_reloaded.saturating_add(other.sources_reloaded);
        self.draft_records_collected = self
            .draft_records_collected
            .saturating_add(other.draft_records_collected);
        self.records_validated = self
            .records_validated
            .saturating_add(other.records_validated);
        self.records_materialized = self
            .records_materialized
            .saturating_add(other.records_materialized);
        self.records_reused = self.records_reused.saturating_add(other.records_reused);
        self.ref_edges_rebuilt = self
            .ref_edges_rebuilt
            .saturating_add(other.ref_edges_rebuilt);
        self.spread_edges_rebuilt = self
            .spread_edges_rebuilt
            .saturating_add(other.spread_edges_rebuilt);
        self.check_roots_executed = self
            .check_roots_executed
            .saturating_add(other.check_roots_executed);
        self.dimension_records_projected = self
            .dimension_records_projected
            .saturating_add(other.dimension_records_projected);
        self.dimension_sources_planned = self
            .dimension_sources_planned
            .saturating_add(other.dimension_sources_planned);
        self.dimension_sources_written = self
            .dimension_sources_written
            .saturating_add(other.dimension_sources_written);
        if let Some(reason) = other.fallback_reason {
            self.mark_full_fallback(reason);
        }
    }

    pub(crate) fn mark_full_fallback(&mut self, reason: IncrementalFallbackReason) {
        self.full_fallback = true;
        self.fallback_reason.get_or_insert(reason);
    }
}
