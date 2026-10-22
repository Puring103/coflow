//! 输出裁剪只处理执行元数据，业务值中的 None、空集合和源码保持原样。
use super::Response;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DiagnosticsSummary {
    pub total: usize,
    pub returned: usize,
    pub truncated: bool,
}

impl Response {
    /// 为终端或 agent 准备有界输出；详情模式保留全部已请求的执行信息。
    pub fn prepare_output(&mut self, details: bool) {
        let reported = self
            .result
            .get("diagnostic_count")
            .or_else(|| self.result.get("validation")?.get("diagnostic_count"))
            .and_then(Value::as_u64)
            .unwrap_or(0) as usize;
        let total = self.diagnostics.len().max(reported).max(
            self.diagnostics_summary
                .as_ref()
                .map_or(0, |summary| summary.total),
        );
        if !details {
            self.diagnostics.truncate(50);
        }
        self.diagnostics_summary = (total > 0).then_some(DiagnosticsSummary {
            total,
            returned: self.diagnostics.len(),
            truncated: total > self.diagnostics.len(),
        });
        if details {
            return;
        }
        if let Some(scope) = &mut self.data_scope {
            // 错误原因已经由 diagnostics 给出；默认只保留加载范围计数，避免再次列出整个项目。
            scope.details_omitted |= !scope.selected_files.is_empty()
                || !scope.incomplete_files.is_empty()
                || !scope.rejected_records.is_empty();
            scope.selected_files.clear();
            scope.incomplete_files.clear();
            scope.rejected_records.clear();
        }
        match self.command.as_str() {
            "check" | "schema.check" => {
                if self.result.get("validation").is_some() {
                    compact_transaction(&mut self.result);
                } else {
                    compact_validation(&mut self.result);
                }
            }
            "apply"
            | "source.write"
            | "source.create"
            | "source.delete"
            | "project.add-input"
            | "project.remove-input"
            | "data.create"
            | "data.set"
            | "data.unset"
            | "data.rename"
            | "data.delete"
            | "data.transfer"
            | "dimension.set"
            | "dimension.clear" => compact_transaction(&mut self.result),
            "build" | "diff" => {
                if let Some(validation) = self.result.get_mut("validation") {
                    compact_validation(validation);
                }
            }
            _ => {}
        }
    }
}

fn compact_validation(report: &mut Value) {
    let Some(report) = report.as_object_mut() else {
        return;
    };
    report.remove("tasks");
    // 汇总保留全部状态计数；失败细节由有界的 diagnostics 给出。
    if let Some(Value::Array(results)) = report.remove("results") {
        if !report.contains_key("rule_counts") {
            report.insert("rule_counts".into(), json!(counts(&results, "state")));
        }
    }
}

fn compact_transaction(report: &mut Value) {
    if let Some(validation) = report.get_mut("validation") {
        compact_validation(validation);
    }
    let summarized = report.get("operation_counts").is_some();
    if let Some(operations) = report.get_mut("operations").and_then(Value::as_array_mut) {
        let summary = counts(operations, "status");
        // 操作序号对应提交数组。保留失败项的原始目标与条件，成功及未执行项用计数表达。
        operations.retain(|operation| operation["status"] == "failed");
        if !summarized {
            report["operation_counts"] = json!(summary);
        }
    }
    if let Some(object) = report.as_object_mut() {
        // files 已提供审阅所需的源码补丁，按字段的完整语义差异由 --details 或 diff 查询。
        object.remove("diff");
        object.remove("endpoints");
    }
}

fn counts(items: &[Value], field: &str) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::from([("total".into(), items.len())]);
    for item in items {
        if let Some(state) = item[field].as_str() {
            *counts.entry(state.into()).or_default() += 1;
        }
    }
    counts
}
