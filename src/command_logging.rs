//! CLI 单次运行的统一终结日志；只保存受控诊断元数据。
use coflow_runtime::automation::Response;
use std::{collections::BTreeSet, time::Instant};

pub(crate) struct CommandLog {
    started: Instant,
    code: u8,
    diagnostic_count: usize,
    codes: BTreeSet<String>,
    stages: BTreeSet<String>,
    written: bool,
    written_files: usize,
    restored_files: usize,
    restoration_failed: usize,
}

impl CommandLog {
    pub(crate) fn new() -> Self {
        coflow_observability::tracing::info!(event = "command_started");
        Self {
            started: Instant::now(),
            code: 4,
            diagnostic_count: 0,
            codes: BTreeSet::new(),
            stages: BTreeSet::new(),
            written: false,
            written_files: 0,
            restored_files: 0,
            restoration_failed: 0,
        }
    }

    pub(crate) fn response(&mut self, response: &Response) {
        self.code = response.exit_code();
        self.diagnostic_count = response.diagnostics.len();
        for diagnostic in &response.diagnostics {
            // 输出截断不影响总数；日志集合有界，避免大量诊断放大单条事件。
            if self.codes.len() < 32 {
                self.codes.insert(diagnostic.code.clone());
            }
            if self.stages.len() < 16 {
                self.stages.insert(diagnostic.stage.clone());
            }
        }
        self.written =
            response.result["written"] == true || response.result["artifacts"]["written"] == true;
        let count = |key: &str| {
            response.result[key]
                .as_array()
                .or_else(|| response.result["artifacts"][key].as_array())
                .map_or(0, Vec::len)
        };
        self.written_files = count("written_files");
        self.restored_files = count("restored_files");
        self.restoration_failed = count("restoration_failed");
    }

    pub(crate) const fn exit(&mut self, code: u8) {
        self.code = code;
    }

    pub(crate) fn output_failed(&mut self) {
        self.code = 4;
        self.diagnostic_count += 1;
        self.codes.insert("CLI-OUTPUT".into());
        self.stages.insert("CLI".into());
    }
}

impl Drop for CommandLog {
    fn drop(&mut self) {
        // RAII 覆盖参数错误、LSP、输出失败与正常退出，不依赖 run 是否返回 Err。
        coflow_observability::tracing::info!(event = "command_finished", exit_code = self.code,
            diagnostic_count = self.diagnostic_count, codes = ?self.codes, stages = ?self.stages,
            written = self.written, written_files = self.written_files,
            restored_files = self.restored_files, restoration_failed = self.restoration_failed,
            elapsed_ms = u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publication_counts_include_nested_codegen_artifacts() {
        let mut response = Response::new("codegen");
        response.result = serde_json::json!({"artifacts": {"written": true,
            "written_files": ["a.cs", "b.cs"], "restored_files": ["a.cs"], "restoration_failed": []}});
        let mut log = CommandLog::new();
        log.response(&response);
        assert!(log.written);
        assert_eq!(log.written_files, 2);
        assert_eq!(log.restored_files, 1);
        assert_eq!(log.restoration_failed, 0);
    }
}
