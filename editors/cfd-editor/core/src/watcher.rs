use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::Serialize;
use coflow_runtime::FlatDiagnostic;

use crate::editor::{EditorError, ProjectBootstrap, SessionStore};

const DEBOUNCE: Duration = Duration::from_millis(350);

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", content = "payload", rename_all = "snake_case")]
pub enum EditorEvent {
    ProjectReloaded(ProjectReloadedPayload),
    ProjectWatchError(ProjectWatchErrorPayload),
}

pub trait EditorEventSink: Send + Sync + 'static {
    fn emit(&self, event: EditorEvent);
}

#[derive(Debug, Default)]
pub struct NoopEditorEventSink;

impl EditorEventSink for NoopEditorEventSink {
    fn emit(&self, _event: EditorEvent) {}
}

#[derive(Debug, Default)]
pub(crate) struct ProjectWatchRegistry {
    watchers: Mutex<HashMap<u32, RecommendedWatcher>>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProjectReloadedPayload {
    pub session_id: u32,
    pub changed_paths: Vec<String>,
    pub revision: u32,
    pub diagnostics: Vec<FlatDiagnostic>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProjectWatchErrorPayload {
    pub session_id: u32,
    pub message: String,
}

impl ProjectWatchRegistry {
    pub(crate) fn watch_session(
        &self,
        sessions: Arc<SessionStore>,
        events: Arc<dyn EditorEventSink>,
        bootstrap: &ProjectBootstrap,
    ) -> Result<(), EditorError> {
        let session_id = bootstrap.session_id;
        let project_root = PathBuf::from(&bootstrap.project_root);
        let (tx, rx) = mpsc::channel::<notify::Result<Event>>();
        let mut watcher = RecommendedWatcher::new(
            move |result| {
                let _ = tx.send(result);
            },
            Config::default(),
        )
        .map_err(|err| EditorError::other(format!("failed to create file watcher: {err}")))?;
        watcher
            .watch(&project_root, RecursiveMode::Recursive)
            .map_err(|err| {
                EditorError::other(format!(
                    "failed to watch project root `{}`: {err}",
                    project_root.display()
                ))
            })?;

        self.watchers
            .lock()
            .map_err(|_| EditorError::session("project watcher registry poisoned"))?
            .insert(session_id, watcher);

        std::thread::spawn(move || watch_loop(&sessions, &events, session_id, &rx));
        Ok(())
    }

    pub(crate) fn unwatch_session(&self, session_id: u32) {
        if let Ok(mut watchers) = self.watchers.lock() {
            watchers.remove(&session_id);
        }
    }
}

fn watch_loop(
    sessions: &SessionStore,
    events: &Arc<dyn EditorEventSink>,
    session_id: u32,
    rx: &mpsc::Receiver<notify::Result<Event>>,
) {
    let mut pending_paths: Vec<PathBuf> = Vec::new();
    while let Ok(result) = rx.recv() {
        match result {
            Ok(event) => {
                if !is_relevant_event(&event) {
                    continue;
                }
                pending_paths.extend(event.paths);
                loop {
                    match rx.recv_timeout(DEBOUNCE) {
                        Ok(Ok(event)) if is_relevant_event(&event) => {
                            pending_paths.extend(event.paths);
                        }
                        Ok(Ok(_)) => {}
                        Ok(Err(err)) => emit_watch_error(events, session_id, err.to_string()),
                        Err(RecvTimeoutError::Timeout) => {
                            let relevant_paths = filter_relevant_paths(&pending_paths);
                            let changed_paths = normalize_paths(&relevant_paths);
                            let external = sessions
                                .has_external_file_changes(session_id, &relevant_paths);
                            pending_paths.clear();
                            match external {
                                Ok(false) => break,
                                Ok(true) => emit_reload(
                                    sessions,
                                    events,
                                    session_id,
                                    changed_paths,
                                ),
                                Err(err) => emit_watch_error(events, session_id, err.message),
                            }
                            break;
                        }
                        Err(RecvTimeoutError::Disconnected) => return,
                    }
                }
            }
            Err(err) => emit_watch_error(events, session_id, err.to_string()),
        }
    }
}

fn is_relevant_event(event: &Event) -> bool {
    if matches!(event.kind, EventKind::Access(_)) {
        return false;
    }
    event.paths.iter().any(|path| !is_ignored_path(path))
}

fn is_ignored_path(path: &Path) -> bool {
    path.components().any(|component| {
        let name = component.as_os_str().to_string_lossy();
        if name.starts_with(".atomicwrite")
            || (name.starts_with('.')
                && (name.contains(".coflow-staging-") || name.contains(".coflow-backup-")))
        {
            return true;
        }
        matches!(
            name.as_ref(),
            ".git"
                | ".coflow"
                | ".idea"
                | ".vscode"
                | "node_modules"
                | "target"
                | "dist"
                | "build"
                | ".next"
                | ".nuxt"
                | ".svelte-kit"
                | "coverage"
                | ".DS_Store"
                | "editor-setting"
        )
    })
}

fn filter_relevant_paths(paths: &[PathBuf]) -> Vec<PathBuf> {
    paths
        .iter()
        .filter(|path| !is_ignored_path(path))
        .cloned()
        .collect()
}

fn normalize_paths(paths: &[PathBuf]) -> Vec<String> {
    let mut out = paths
        .iter()
        .filter(|path| !is_ignored_path(path))
        .map(|path| path.display().to_string().replace('\\', "/"))
        .collect::<Vec<_>>();
    out.sort();
    out.dedup();
    out
}

fn emit_reload(
    sessions: &SessionStore,
    events: &Arc<dyn EditorEventSink>,
    session_id: u32,
    changed_paths: Vec<String>,
) {
    match sessions.reload_session(session_id) {
        Ok(snapshot) => events.emit(EditorEvent::ProjectReloaded(ProjectReloadedPayload {
            session_id,
            changed_paths,
            revision: snapshot.revision,
            diagnostics: snapshot.diagnostics,
        })),
        Err(err) => emit_watch_error(events, session_id, err.message),
    }
}

fn emit_watch_error(events: &Arc<dyn EditorEventSink>, session_id: u32, message: String) {
    events.emit(EditorEvent::ProjectWatchError(ProjectWatchErrorPayload {
        session_id,
        message,
    }));
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::filter_relevant_paths;

    #[test]
    fn project_state_directory_does_not_trigger_reload() {
        let paths = vec![
            PathBuf::from("project/.coflow/editor.json"),
            PathBuf::from("project/data/items.cfd"),
        ];

        assert_eq!(
            filter_relevant_paths(&paths),
            vec![PathBuf::from("project/data/items.cfd")]
        );
    }

    #[test]
    fn atomic_write_staging_paths_do_not_trigger_reload() {
        let paths = vec![
            PathBuf::from("project/data/.items.cfd.coflow-staging-1-2-3"),
            PathBuf::from("project/data/.items.cfd.coflow-backup-1-2-4"),
            PathBuf::from("project/data/.atomicwriteAbCd/source"),
            PathBuf::from("project/data/items.cfd"),
        ];

        assert_eq!(
            filter_relevant_paths(&paths),
            vec![PathBuf::from("project/data/items.cfd")]
        );
    }
}
