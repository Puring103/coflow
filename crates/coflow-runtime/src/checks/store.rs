use crate::data_model::CfdDataModel;
use coflow_core::check::CheckDiagnostic;

/// 当前只保存执行入口诊断；规则执行尚未建立语句缓存或调度索引。
#[derive(Debug, Clone, Default)]
pub(crate) struct CheckDiagnosticStore {
    pub(super) request_diagnostics: Vec<CheckDiagnostic>,
}

impl CheckDiagnosticStore {
    pub(super) fn diagnostics(&self, _model: &CfdDataModel) -> Vec<CheckDiagnostic> {
        self.request_diagnostics.clone()
    }
}
