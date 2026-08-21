use crate::checker::{CheckDiagnostic, CheckTask};

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
    pub request_diagnostics: Vec<CheckDiagnostic>,
    pub statistics: CheckExecutionStats,
}

impl CheckOutput {
    #[must_use]
    pub fn is_success(&self) -> bool {
        self.request_diagnostics.is_empty()
            && self
                .results
                .iter()
                .all(|result| result.diagnostics.is_empty())
    }

    pub fn diagnostics(&self) -> impl Iterator<Item = &CheckDiagnostic> {
        self.request_diagnostics
            .iter()
            .chain(self.results.iter().flat_map(|result| &result.diagnostics))
    }
}
