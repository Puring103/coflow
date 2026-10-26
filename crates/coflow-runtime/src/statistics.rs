/// Deterministic work counters for the latest immutable project generation.
///
/// These counters describe actual runtime work and are returned separately
/// from configuration statistics when a host requests performance information.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub struct ProjectExecutionStats {
    pub sources_resolved: usize,
    pub sources_reloaded: usize,
    pub draft_records_collected: usize,
    pub records_validated: usize,
    pub identity_records_checked: usize,
    pub origin_records_checked: usize,
    pub records_materialized: usize,
    pub records_reused: usize,
    pub ref_edges_rebuilt: usize,
    pub check_roots_executed: usize,
    pub dimension_sources_planned: usize,
    pub dimension_sources_written: usize,
}
