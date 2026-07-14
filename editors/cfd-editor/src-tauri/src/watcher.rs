use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::Mutex;
use std::time::Duration;

use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::editor::{EditorError, ProjectSnapshot};
use crate::host::EditorHost;

const PROJECT_CHANGED_EVENT: &str = "project_changed";
const PROJECT_WATCH_ERROR_EVENT: &str = "project_watch_error";
const DEBOUNCE: Duration = Duration::from_millis(350);

#[derive(Debug, Default)]
pub struct ProjectWatchRegistry {
    watchers: Mutex<HashMap<u32, RecommendedWatcher>>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProjectChangedPayload {
    pub session_id: u32,
    pub changed_paths: Vec<String>,
    pub snapshot: ProjectSnapshot,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProjectWatchErrorPayload {
    pub session_id: u32,
    pub message: String,
}

impl ProjectWatchRegistry {
    pub fn watch_session(
        &self,
        app: AppHandle,
        snapshot: &ProjectSnapshot,
    ) -> Result<(), EditorError> {
        let session_id = snapshot.session_id;
        let project_root = PathBuf::from(&snapshot.project_root);
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

        std::thread::spawn(move || watch_loop(&app, session_id, &rx));
        Ok(())
    }

    pub fn unwatch_session(&self, session_id: u32) {
        if let Ok(mut watchers) = self.watchers.lock() {
            watchers.remove(&session_id);
        }
    }
}

fn watch_loop(app: &AppHandle, session_id: u32, rx: &mpsc::Receiver<notify::Result<Event>>) {
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
                        Ok(Err(err)) => emit_watch_error(app, session_id, err.to_string()),
                        Err(RecvTimeoutError::Timeout) => {
                            let relevant_paths = filter_relevant_paths(&pending_paths);
                            let changed_paths = normalize_paths(&relevant_paths);
                            let external = has_external_changes(app, session_id, &relevant_paths);
                            pending_paths.clear();
                            match external {
                                Ok(false) => break,
                                Ok(true) => emit_reload(app, session_id, changed_paths),
                                Err(err) => emit_watch_error(app, session_id, err.message),
                            }
                            break;
                        }
                        Err(RecvTimeoutError::Disconnected) => return,
                    }
                }
            }
            Err(err) => emit_watch_error(app, session_id, err.to_string()),
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
    if path
        .extension()
        .is_some_and(|extension| extension.to_string_lossy().eq_ignore_ascii_case("xlsxtmp"))
    {
        return true;
    }
    path.components().any(|component| {
        let name = component.as_os_str().to_string_lossy();
        matches!(
            name.as_ref(),
            ".git"
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
        )
    })
}

pub(crate) fn filter_relevant_paths(paths: &[PathBuf]) -> Vec<PathBuf> {
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

fn emit_reload(app: &AppHandle, session_id: u32, changed_paths: Vec<String>) {
    let host = app.state::<EditorHost>();
    match host.reload_session(session_id) {
        Ok(snapshot) => {
            let _ = app.emit(
                PROJECT_CHANGED_EVENT,
                ProjectChangedPayload {
                    session_id,
                    changed_paths,
                    snapshot,
                },
            );
        }
        Err(err) => emit_watch_error(app, session_id, err.message),
    }
}

fn has_external_changes(
    app: &AppHandle,
    session_id: u32,
    changed_paths: &[PathBuf],
) -> Result<bool, EditorError> {
    app.state::<EditorHost>()
        .has_external_file_changes(session_id, changed_paths)
}

fn emit_watch_error(app: &AppHandle, session_id: u32, message: String) {
    let _ = app.emit(
        PROJECT_WATCH_ERROR_EVENT,
        ProjectWatchErrorPayload {
            session_id,
            message,
        },
    );
}
