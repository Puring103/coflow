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
    host: State<'_, EditorHost>,
    work: impl FnOnce(EditorHost) -> Result<T, EditorError> + Send + 'static,
) -> Result<T, EditorError>
where
    T: Send + 'static,
{
    // Tauri State 的借用不能进入阻塞线程，统一在边界处克隆线程安全的 host。
    let host = host.inner().clone();
    run_blocking(move || work(host)).await
}
