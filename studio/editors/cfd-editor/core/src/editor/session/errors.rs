//! 会话错误映射：把引擎诊断统一转成前端可路由的 `EditorError`。
//!
//! 后端是诊断文本归一的唯一位置，前端只消费 `kind/message/diagnostics`。

use crate::editor::types::EditorError;

// 统一的诊断文本拼接 + `flat_view` 收集，避免三处手写 `join("; ")`。
fn join_messages(messages: &[&str]) -> String {
    messages.join("; ")
}

fn flat_views(diagnostics: &coflow_project::DiagnosticSet) -> Vec<coflow_project::FlatDiagnostic> {
    diagnostics
        .diagnostics
        .iter()
        .map(|d| d.flat_view(None, None, None))
        .collect()
}

/// 统一入口：按 `kind` 构造错误，避免调用方手写拼接逻辑。
pub(crate) fn diagnostics_to_error(
    kind: crate::editor::types::EditorErrorKind,
    diagnostics: &coflow_project::DiagnosticSet,
) -> EditorError {
    let message = join_messages(
        &diagnostics
            .iter()
            .map(|d| d.message.as_str())
            .collect::<Vec<_>>(),
    );
    EditorError::new(kind, message).with_diagnostics(flat_views(diagnostics))
}

pub(crate) fn api_diagnostics_to_editor_error(
    diagnostics: coflow_project::DiagnosticSet,
) -> EditorError {
    diagnostics_to_error(crate::editor::types::EditorErrorKind::Write, &diagnostics)
}

pub(crate) fn project_diagnostics_to_editor_error(
    diagnostics: &coflow_project::DiagnosticSet,
) -> EditorError {
    diagnostics_to_error(crate::editor::types::EditorErrorKind::Project, diagnostics)
}

/// mutation 上报的失败诊断合并：`failed.*.diagnostics` + `report.diagnostics`。
pub(crate) fn mutation_report_to_editor_error(
    fallback: &str,
    report: &coflow_project::MutationReport,
) -> EditorError {
    let message = join_messages(
        &report
            .failed
            .iter()
            .flat_map(|failed| failed.diagnostics.iter())
            .map(|diagnostic| diagnostic.message.as_str())
            .collect::<Vec<_>>(),
    );
    let diagnostics = report
        .failed
        .iter()
        .flat_map(|failed| failed.diagnostics.iter().cloned())
        .chain(report.diagnostics.iter().cloned())
        .collect();
    EditorError::write(if message.is_empty() {
        fallback.to_string()
    } else {
        message
    })
    .with_diagnostics(diagnostics)
}
