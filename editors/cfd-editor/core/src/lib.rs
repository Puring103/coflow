//! Host-independent CFD editor backend.

#![allow(clippy::missing_errors_doc, clippy::module_name_repetitions)]

pub mod editor;
mod host;
mod watcher;

pub use editor::{EditorError, ProjectBootstrap, SessionStore};
pub use host::EditorHost;
pub use watcher::{
    EditorEvent, EditorEventSink, NoopEditorEventSink, ProjectReloadedPayload,
    ProjectWatchErrorPayload,
};
