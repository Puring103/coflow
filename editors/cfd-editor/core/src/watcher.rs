use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::Arc;
use std::time::Duration;

use coflow_runtime::Project;
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use parking_lot::Mutex;
use serde::Serialize;

use crate::editor::{EditorError, ProjectBootstrap, SessionStore};

const DEBOUNCE: Duration = Duration::from_millis(350);

/// 前端监听的 Tauri 事件名。核心定义一次，主机层与前端只引用此处。
pub const PROJECT_RELOADED_EVENT: &str = "project_reloaded";
pub const PROJECT_WATCH_ERROR_EVENT: &str = "project_watch_error";

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
    watchers: Mutex<HashMap<u32, ProjectWatcher>>,
}

#[derive(Debug)]
struct ProjectWatcher {
    watcher: RecommendedWatcher,
    roots: Vec<PathBuf>,
}

#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        rename = "ProjectReloadedEvent",
        export,
        export_to = "../../frontend/src/bindings/"
    )
)]
pub struct ProjectReloadedPayload {
    pub session_id: u32,
    pub changed_paths: Vec<String>,
    pub bootstrap: ProjectBootstrap,
}

#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        rename = "ProjectWatchErrorEvent",
        export,
        export_to = "../../frontend/src/bindings/"
    )
)]
pub struct ProjectWatchErrorPayload {
    pub session_id: u32,
    pub error: EditorError,
}

impl ProjectWatchRegistry {
    pub(crate) fn watch_session(
        self: &Arc<Self>,
        sessions: Arc<SessionStore>,
        events: Arc<dyn EditorEventSink>,
        bootstrap: &ProjectBootstrap,
    ) -> Result<(), EditorError> {
        let session_id = bootstrap.session_id;
        let project_root = PathBuf::from(&bootstrap.project_root);
        let project = Project::open_schema_only(Some(&project_root)).map_err(|diagnostics| {
            crate::editor::project_diagnostics_to_editor_error(&diagnostics)
        })?;
        let (tx, rx) = mpsc::channel::<notify::Result<Event>>();
        let mut watcher = RecommendedWatcher::new(
            move |result| {
                let _ = tx.send(result);
            },
            Config::default(),
        )
        .map_err(|err| EditorError::other(format!("failed to create file watcher: {err}")))?;
        let roots = watch_roots(&project);
        for path in &roots {
            watch_path(&mut watcher, path)?;
        }

        self.watchers
            .lock()
            .insert(session_id, ProjectWatcher { watcher, roots });

        let registry = Arc::downgrade(self);
        std::thread::spawn(move || {
            watch_loop(
                &sessions,
                &events,
                &registry,
                session_id,
                &project_root,
                &rx,
            );
        });
        Ok(())
    }

    pub(crate) fn unwatch_session(&self, session_id: u32) {
        self.watchers.lock().remove(&session_id);
    }

    fn refresh_session(&self, session_id: u32, project_root: &Path) -> Result<(), EditorError> {
        let project = Project::open_schema_only(Some(project_root)).map_err(|diagnostics| {
            EditorError::project(format!(
                "failed to refresh project watch paths: {diagnostics}"
            ))
        })?;
        let next_roots = watch_roots(&project);
        let mut watchers = self.watchers.lock();
        let Some(entry) = watchers.get_mut(&session_id) else {
            return Ok(());
        };
        if entry.roots == next_roots {
            return Ok(());
        }

        // 先注册新增路径；注册失败时保留原监听集合，避免会话失去文件更新。
        for path in next_roots.iter().filter(|path| !entry.roots.contains(path)) {
            watch_path(&mut entry.watcher, path)?;
        }
        for path in entry.roots.iter().filter(|path| !next_roots.contains(path)) {
            entry.watcher.unwatch(path).map_err(|err| {
                EditorError::other(format!(
                    "failed to unwatch project source `{}`: {err}",
                    path.display()
                ))
            })?;
        }
        entry.roots = next_roots;
        drop(watchers);
        Ok(())
    }
}

fn watch_path(watcher: &mut RecommendedWatcher, path: &Path) -> Result<(), EditorError> {
    let mode = if path.is_dir() {
        RecursiveMode::Recursive
    } else {
        RecursiveMode::NonRecursive
    };
    watcher.watch(path, mode).map_err(|err| {
        EditorError::other(format!(
            "failed to watch project source `{}`: {err}",
            path.display()
        ))
    })
}

fn watch_roots(project: &Project) -> Vec<PathBuf> {
    let project_root = coflow_runtime::normalize_path(project.root_dir());
    let mut roots = vec![project_root.clone()];
    for source_root in project.source_roots() {
        if !coflow_runtime::path_is_same_or_descendant(&source_root, &project_root) {
            // 维度目录可尚未创建，先监听最近存在的父目录以捕获首次创建。
            if let Some(existing) = source_root.ancestors().find(|path| path.exists()) {
                roots.push(existing.to_path_buf());
            }
        }
    }
    roots.sort();
    roots.dedup();
    roots
}

fn watch_loop(
    sessions: &SessionStore,
    events: &Arc<dyn EditorEventSink>,
    registry: &std::sync::Weak<ProjectWatchRegistry>,
    session_id: u32,
    project_root: &Path,
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
                            let external =
                                sessions.has_external_file_changes(session_id, &relevant_paths);
                            pending_paths.clear();
                            match external {
                                Ok(false) => break,
                                Ok(true) => {
                                    if emit_reload(sessions, events, session_id, &relevant_paths) {
                                        if let Some(registry) = registry.upgrade() {
                                            if let Err(err) =
                                                registry.refresh_session(session_id, project_root)
                                            {
                                                emit_watch_error(events, session_id, err.message);
                                            }
                                        }
                                    }
                                }
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
    // 输入根由项目配置确定，不能因绝对路径的祖先名为 build/target 而丢弃事件。
    path.components().any(|component| {
        let name = component.as_os_str().to_string_lossy();
        name.starts_with(".atomicwrite")
            || (name.starts_with('.')
                && (name.contains(".coflow-staging-") || name.contains(".coflow-backup-")))
            || matches!(
                name.as_ref(),
                ".coflow" | ".git" | "editor-setting" | ".DS_Store"
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
        .map(|path| coflow_runtime::path_to_slash(&coflow_runtime::normalize_path(path)))
        .collect::<Vec<_>>();
    out.sort();
    out.dedup();
    out
}

fn emit_reload(
    sessions: &SessionStore,
    events: &Arc<dyn EditorEventSink>,
    session_id: u32,
    changed_paths: &[PathBuf],
) -> bool {
    match sessions.reload_session_paths(session_id, changed_paths) {
        Ok(snapshot) => {
            events.emit(EditorEvent::ProjectReloaded(ProjectReloadedPayload {
                session_id,
                changed_paths: normalize_paths(changed_paths),
                bootstrap: snapshot,
            }));
            true
        }
        Err(err) => {
            events.emit(EditorEvent::ProjectWatchError(ProjectWatchErrorPayload {
                session_id,
                error: err,
            }));
            false
        }
    }
}

fn emit_watch_error(events: &Arc<dyn EditorEventSink>, session_id: u32, message: String) {
    events.emit(EditorEvent::ProjectWatchError(ProjectWatchErrorPayload {
        session_id,
        error: EditorError::other(message),
    }));
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{filter_relevant_paths, watch_roots};
    use coflow_runtime::Project;

    #[derive(Default)]
    struct Events(parking_lot::Mutex<Vec<super::EditorEvent>>);
    impl super::EditorEventSink for Events {
        fn emit(&self, event: super::EditorEvent) {
            self.0.lock().push(event);
        }
    }

    #[test]
    fn reload_event_contains_committed_generation_and_structured_schema_errors() {
        let root = tempfile::tempdir().expect("project");
        let schema = root.path().join("schema.cft");
        let data = root.path().join("data.cfd");
        std::fs::write(root.path().join("coflow.yaml"), "schema: schema.cft\ndata: data.cfd\ncodegen:\n  - language: csharp\n    dir: generated/\n").expect("config");
        std::fs::write(&schema, "table Item { value: int; }").expect("schema");
        std::fs::write(&data, "a: Item { value: 1 }").expect("data");
        let sessions = super::SessionStore::new().expect("store");
        let before = sessions
            .load_project(&root.path().join("coflow.yaml"))
            .expect("load");
        let captured = std::sync::Arc::new(Events::default());
        let events: std::sync::Arc<dyn super::EditorEventSink> = captured.clone();
        std::fs::write(&data, "a: Item { value: 2 }").expect("edit data");
        assert!(super::emit_reload(
            &sessions,
            &events,
            before.session_id,
            &[data]
        ));
        match &captured.0.lock()[0] {
            super::EditorEvent::ProjectReloaded(event) => {
                assert_eq!(event.bootstrap.revision, before.revision + 1);
                assert_eq!(event.bootstrap.session_id, before.session_id);
            }
            _ => panic!("expected reload"),
        }
        std::fs::write(&schema, "table Item { value: MissingType; }").expect("edit schema");
        assert!(!super::emit_reload(
            &sessions,
            &events,
            before.session_id,
            &[schema]
        ));
        match &captured.0.lock()[1] {
            super::EditorEvent::ProjectWatchError(event) => {
                assert!(!event.error.diagnostics.is_empty());
                assert!(event
                    .error
                    .diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.code != "EDITOR-INTERNAL"));
            }
            _ => panic!("expected structured error"),
        };
    }

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

    #[test]
    fn configured_external_source_is_watched() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before unix epoch")
            .as_nanos();
        let temp = std::env::temp_dir().join(format!("coflow-watch-roots-{nonce}"));
        let project_root = temp.join("project");
        let external = temp.join("shared");
        std::fs::create_dir_all(project_root.join("schema")).expect("create schema directory");
        std::fs::create_dir_all(&external).expect("create external data directory");
        std::fs::write(project_root.join("schema/main.cft"), "table Item {}\n")
            .expect("write schema");
        std::fs::write(
            project_root.join("coflow.yaml"),
            "schema: schema/\ndata: ../shared/\ncodegen:\n  - language: csharp\n    dir: generated/\n",
        )
        .expect("write project config");

        let project = Project::open_schema_only(Some(&project_root)).expect("open project");
        assert_eq!(
            watch_roots(&project),
            vec![
                coflow_runtime::normalize_path(&project_root),
                coflow_runtime::normalize_path(&external),
            ]
        );
        std::fs::remove_dir_all(&temp).expect("remove temp directory");
    }
}

#[cfg(test)]
mod path_tests {
    use super::*;
    #[test]
    fn configured_inputs_under_build_named_ancestors_are_relevant() {
        assert!(!is_ignored_path(Path::new(
            "C:/build/project/target/items.cfd"
        )));
        assert!(!is_ignored_path(Path::new("/external/dist/schema.cft")));
        assert!(is_ignored_path(Path::new(
            "/project/.items.cfd.coflow-staging-1"
        )));
    }
}
