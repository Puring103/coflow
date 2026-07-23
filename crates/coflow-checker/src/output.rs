use coflow_data_model::{CfdDiagnostics, CfdRecordId};

use crate::{CheckDiagnostic, CheckSnapshot, DependencyCollection, DependencyGraph};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootedCheckDiagnostic {
    pub root: CfdRecordId,
    pub diagnostic: CheckDiagnostic,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CheckExecutionStats {
    pub requested_roots: usize,
    pub executed_rounds: usize,
    pub dimension_projected_records: usize,
    pub dependency_collection: DependencyCollection,
}

#[derive(Debug, Clone, Default)]
pub struct CheckOutput {
    pub diagnostics: Vec<RootedCheckDiagnostic>,
    pub dependencies: DependencyGraph,
    pub statistics: CheckExecutionStats,
    pub snapshot: Option<CheckSnapshot>,
}

impl CheckOutput {
    #[must_use]
    pub fn is_success(&self) -> bool {
        self.diagnostics.is_empty()
    }

    /// Converts failed checks into the aggregate diagnostic result.
    ///
    /// # Errors
    ///
    /// Returns all check diagnostics when any requested root failed.
    pub fn into_result(self) -> Result<(), CfdDiagnostics> {
        if self.diagnostics.is_empty() {
            Ok(())
        } else {
            Err(CfdDiagnostics::new(
                self.diagnostics
                    .into_iter()
                    .map(|rooted| rooted.diagnostic.into_legacy_diagnostic())
                    .collect(),
            ))
        }
    }
}
