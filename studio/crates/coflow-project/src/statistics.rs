/// Deterministic work counters for the latest immutable project generation.
///
/// These counters describe actual runtime work. They are deliberately kept
/// outside editor and mutation wire DTOs so observability does not alter
/// their serialized contracts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProjectExecutionStats {
    pub sources_resolved: usize,
    pub sources_reloaded: usize,
    pub draft_records_collected: usize,
    pub records_validated: usize,
    pub records_materialized: usize,
    pub records_reused: usize,
    pub ref_edges_rebuilt: usize,
    pub check_roots_executed: usize,
    pub dimension_records_projected: usize,
}
