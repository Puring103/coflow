use cfd_editor_core::{
    EditorEvent, EditorEventSink, PROJECT_RELOADED_EVENT, PROJECT_WATCH_ERROR_EVENT,
};
use tauri::{AppHandle, Emitter};

#[derive(Debug, Clone)]
pub struct TauriEditorEventSink {
    app: AppHandle,
}

impl TauriEditorEventSink {
    pub const fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

impl EditorEventSink for TauriEditorEventSink {
    fn emit(&self, event: EditorEvent) {
        // 主机层只负责把核心事件名映射为 Tauri 事件，不承载编辑器业务逻辑。
        match event {
            EditorEvent::ProjectReloaded(payload) => {
                if self.app.emit(PROJECT_RELOADED_EVENT, payload).is_err() {
                    coflow_observability::tracing::error!(
                        event = "event_delivery_failed",
                        name = PROJECT_RELOADED_EVENT
                    );
                }
            }
            EditorEvent::ProjectWatchError(payload) => {
                coflow_observability::tracing::warn!(
                    event = "project_watch_error",
                    session_id = payload.session_id
                );
                if self.app.emit(PROJECT_WATCH_ERROR_EVENT, payload).is_err() {
                    coflow_observability::tracing::error!(
                        event = "event_delivery_failed",
                        name = PROJECT_WATCH_ERROR_EVENT
                    );
                }
            }
        }
    }
}
