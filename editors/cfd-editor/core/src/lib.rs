//! Host-independent CFD editor backend.

#![allow(clippy::missing_errors_doc, clippy::module_name_repetitions)]
// 编辑器宿主的文件监听与临时文件依赖仍覆盖新旧 rustix/rand_core 主版本。
#![allow(clippy::multiple_crate_versions)]

pub mod editor;
mod host;
mod watcher;

pub use editor::{EditorError, ProjectBootstrap, SessionStore};
pub use host::EditorHost;
pub use watcher::{
    EditorEvent, EditorEventSink, NoopEditorEventSink, ProjectReloadedPayload,
    ProjectWatchErrorPayload,
};
