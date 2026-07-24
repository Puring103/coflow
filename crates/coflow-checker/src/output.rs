use crate::{CheckDiagnostic, CheckTask};
use coflow_data_model::CfdDiagnostics;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CheckExecutionStats {
    pub requested_tasks: usize,
    pub executed_tasks: usize,
    pub rejected_tasks: usize,
    pub work_used: u64,
    pub dimension_projected_records: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CheckTaskResult {
    pub task: CheckTask,
    pub diagnostics: Vec<CheckDiagnostic>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CheckOutput {
    pub results: Vec<CheckTaskResult>,
    pub statistics: CheckExecutionStats,
}

impl CheckOutput {
    #[must_use]
    pub fn is_success(&self) -> bool {
        self.results
            .iter()
            .all(|result| result.diagnostics.is_empty())
    }

    pub fn diagnostics(&self) -> impl Iterator<Item = &CheckDiagnostic> {
        self.results.iter().flat_map(|result| &result.diagnostics)
    }

    /// Converts failed checks into the aggregate diagnostic result.
    ///
    /// # Errors
    ///
    /// Returns all diagnostics produced by the requested tasks.
    pub fn into_result(self) -> Result<(), CfdDiagnostics> {
        let diagnostics = self
            .results
            .into_iter()
            .flat_map(|result| result.diagnostics)
            .map(CheckDiagnostic::into_legacy_diagnostic)
            .collect::<Vec<_>>();
        if diagnostics.is_empty() {
            Ok(())
        } else {
            Err(CfdDiagnostics::new(diagnostics))
        }
    }
}
