//! Tauri 命令适配层。

mod data;
mod language;
mod project;

pub(crate) use data::*;
pub(crate) use language::*;
pub(crate) use project::*;

use cfd_editor_core::EditorHost;
use tauri::State;

use cfd_editor_core::editor::EditorError;
pub(crate) async fn run_blocking<T>(
    work: impl FnOnce() -> Result<T, EditorError> + Send + 'static,
) -> Result<T, EditorError>
where
    T: Send + 'static,
{
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| EditorError::other(format!("background command failed: {error}")))?
}

pub(crate) async fn run_host_command<T>(
    command: &'static str,
    host: State<'_, EditorHost>,
    work: impl FnOnce(EditorHost) -> Result<T, EditorError> + Send + 'static,
) -> Result<T, EditorError>
where
    T: Send + 'static,
{
    // Tauri State 的借用不能进入阻塞线程，统一在边界处克隆线程安全的 host。
    let host = host.inner().clone();
    let operation_id = coflow_observability::identifier();
    let started = std::time::Instant::now();
    coflow_observability::tracing::info!(run_id = coflow_observability::run_id(), command, %operation_id, event = "command_started");
    // 阻塞线程显式继承操作 span，底层发布事件与界面错误编号保持关联。
    let span = coflow_observability::tracing::info_span!("editor_command", run_id = coflow_observability::run_id(), command, %operation_id);
    let result = run_blocking(move || span.in_scope(|| work(host))).await;
    match result {
        Ok(value) => {
            coflow_observability::tracing::info!(run_id = coflow_observability::run_id(), command, %operation_id, elapsed_ms = started.elapsed().as_millis() as u64, event = "command_completed");
            Ok(value)
        }
        Err(mut error) => {
            error.error_id = Some(operation_id.clone());
            coflow_observability::tracing::error!(run_id = coflow_observability::run_id(), command, error_id = %operation_id, code = %error.code, kind = ?error.kind, persistence = ?error.publication.as_ref().map(|p| &p.state), elapsed_ms = started.elapsed().as_millis() as u64, event = "command_failed");
            Err(error)
        }
    }
}
