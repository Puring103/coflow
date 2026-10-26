//! 本机诊断入口；只读快照交由用户预览和保存，不自动上传。
use cfd_editor_core::editor::EditorError;
use tauri::State;
pub(crate) struct Diagnostics(pub Result<coflow_observability::Logging, String>);
#[tauri::command]
pub(crate) fn diagnostic_snapshot(
    state: State<'_, Diagnostics>,
) -> Result<serde_json::Value, EditorError> {
    match &state.0 {
        Ok(logging) => logging
            .snapshot()
            .map_err(|error| EditorError::other(format!("读取日志失败：{error}"))),
        Err(error) => Ok(serde_json::json!({"logging_error": error})),
    }
}
#[tauri::command]
pub(crate) fn open_log_directory(state: State<'_, Diagnostics>) -> Result<(), EditorError> {
    let logging = state
        .0
        .as_ref()
        .map_err(|error| EditorError::other(error.clone()))?;
    crate::open::open_path(&logging.directory)
}
#[derive(serde::Deserialize)]
pub(crate) struct FrontendEvent {
    pub category: String,
    pub command: Option<String>,
    pub error_id: String,
}
#[tauri::command]
pub(crate) fn record_frontend_events(events: Vec<FrontendEvent>) -> Result<(), EditorError> {
    if events.len() > 32 {
        return Err(EditorError::other("日志批次超过上限"));
    }
    for event in events {
        // 只接收受限标识，不接收异常文本、堆栈、源码或 IPC 参数。
        fn safe(value: &str) -> String {
            value
                .chars()
                .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
                .take(96)
                .collect()
        }
        coflow_observability::tracing::warn!(run_id = coflow_observability::run_id(), event = "frontend_failure", category = %safe(&event.category), command = %safe(event.command.as_deref().unwrap_or("")), error_id = %safe(&event.error_id));
    }
    Ok(())
}

#[tauri::command]
pub(crate) fn open_feedback_page() -> Result<(), EditorError> {
    crate::open::open_path(std::path::Path::new(
        "https://github.com/Puring103/coflow/issues/new",
    ))
}
