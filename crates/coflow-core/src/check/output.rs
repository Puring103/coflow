use super::CheckDiagnostic;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CheckExecutionStats {
    pub requested_tasks: usize,
    pub executed_tasks: usize,
    pub rejected_tasks: usize,
    pub work_used: u64,
    pub dimension_projected_records: usize,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CheckOutput {
    pub request_diagnostics: Vec<CheckDiagnostic>,
    pub statistics: CheckExecutionStats,
}

impl CheckOutput {
    #[must_use]
    pub fn is_success(&self) -> bool {
        self.request_diagnostics.is_empty()
    }
    pub fn diagnostics(&self) -> impl Iterator<Item = &CheckDiagnostic> {
        self.request_diagnostics.iter()
    }
}
