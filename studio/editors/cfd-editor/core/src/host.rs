use std::path::Path;
use std::sync::Arc;

use crate::editor::{EditorError, ProjectBootstrap, SessionStore};
use crate::watcher::{EditorEventSink, ProjectWatchRegistry};

#[derive(Clone)]
pub struct EditorHost {
    sessions: Arc<SessionStore>,
    watchers: Arc<ProjectWatchRegistry>,
    events: Arc<dyn EditorEventSink>,
}

impl std::fmt::Debug for EditorHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EditorHost").finish_non_exhaustive()
    }
}

impl EditorHost {
    pub fn new(events: Arc<dyn EditorEventSink>) -> Result<Self, EditorError> {
        Ok(Self {
            sessions: Arc::new(SessionStore::new()?),
            watchers: Arc::new(ProjectWatchRegistry::default()),
            events,
        })
    }

    pub fn load_project(&self, yaml_path: &Path) -> Result<ProjectBootstrap, EditorError> {
        self.load_project_with_watch(yaml_path, |snapshot| {
            self.watchers.watch_session(
                Arc::clone(&self.sessions),
                Arc::clone(&self.events),
                snapshot,
            )
        })
    }

    pub fn init_project(&self, dir: &Path) -> Result<ProjectBootstrap, EditorError> {
        self.init_project_with_watch(dir, |snapshot| {
            self.watchers.watch_session(
                Arc::clone(&self.sessions),
                Arc::clone(&self.events),
                snapshot,
            )
        })
    }

    pub fn close_session(&self, session_id: u32) -> Result<(), EditorError> {
        self.sessions.close_session(session_id)?;
        self.watchers.unwatch_session(session_id);
        Ok(())
    }

    pub fn reload_session(&self, session_id: u32) -> Result<ProjectBootstrap, EditorError> {
        let result = self.sessions.reload_session(session_id).and_then(|snapshot| {
            self.watchers.refresh_session(session_id, Path::new(&snapshot.project_root))?;
            Ok(snapshot)
        });
        match result {
            Ok(snapshot) => { self.sessions.finish_recovery(session_id); Ok(snapshot) }
            Err(error) => { self.sessions.mark_needs_reload(session_id); Err(error) }
        }
    }

    /// 结构提交显式刷新监听范围，不依赖已过滤的内部文件事件。
    fn finish_structure(&self, id: u32, result: Result<ProjectBootstrap, EditorError>) -> Result<ProjectBootstrap, EditorError> {
        let snapshot = result?;
        if let Err(error) = self.watchers.refresh_session(id, Path::new(&snapshot.project_root)) {
            self.sessions.mark_needs_reload(id);
            return Err(EditorError::new(crate::editor::EditorErrorKind::Committed, format!("操作已完成，但文件监听更新失败，请重新加载项目。{}", error.message)));
        }
        self.sessions.finish_recovery(id);
        Ok(snapshot)
    }
    pub fn add_project_input(&self, id: u32, kind: coflow_project::ProjectInputKind, path: &Path) -> Result<ProjectBootstrap, EditorError> {
        self.finish_structure(id, self.sessions.add_project_input(id, kind, path))
    }
    pub fn create_project_file(&self, id: u32, kind: coflow_project::ProjectInputKind, parent: &Path, name: &str) -> Result<ProjectBootstrap, EditorError> {
        self.finish_structure(id, self.sessions.create_project_file(id, kind, parent, name))
    }
    pub fn delete_project_entry(&self, id: u32, path: &Path) -> Result<ProjectBootstrap, EditorError> {
        self.finish_structure(id, self.sessions.delete_project_entry(id, path))
    }

    #[must_use]
    pub fn sessions(&self) -> &SessionStore {
        &self.sessions
    }

    fn load_project_with_watch<F>(
        &self,
        yaml_path: &Path,
        start_watch: F,
    ) -> Result<ProjectBootstrap, EditorError>
    where
        F: FnOnce(&ProjectBootstrap) -> Result<(), EditorError>,
    {
        let snapshot = self.sessions.load_project(yaml_path)?;
        self.finish_open(snapshot, start_watch)
    }

    fn init_project_with_watch<F>(
        &self,
        dir: &Path,
        start_watch: F,
    ) -> Result<ProjectBootstrap, EditorError>
    where
        F: FnOnce(&ProjectBootstrap) -> Result<(), EditorError>,
    {
        let snapshot = self.sessions.init_project(dir)?;
        self.finish_open(snapshot, start_watch)
    }

    fn finish_open<F>(
        &self,
        bootstrap: ProjectBootstrap,
        start_watch: F,
    ) -> Result<ProjectBootstrap, EditorError>
    where
        F: FnOnce(&ProjectBootstrap) -> Result<(), EditorError>,
    {
        if let Err(error) = start_watch(&bootstrap) {
            self.sessions.close_session(bootstrap.session_id)?;
            return Err(error);
        }
        Ok(bootstrap)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::editor::EditorError;
    use crate::watcher::NoopEditorEventSink;

    use super::EditorHost;

    #[test]
    fn watched_session_rolls_back_when_watcher_start_fails() {
        let root = std::env::temp_dir().join(format!(
            "coflow-editor-host-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("create temp project");
        let host = EditorHost::new(Arc::new(NoopEditorEventSink)).expect("create editor host");

        let error = host
            .init_project_with_watch(&root, |_| Err(EditorError::other("watch failed")))
            .expect_err("watch failure must fail the open operation");

        assert_eq!(error.message, "watch failed");
        assert!(host
            .sessions()
            .get_file_records(1, "data/main.cfd")
            .is_err());
        std::fs::remove_dir_all(root).expect("remove temp project");
    }
    #[test]
    fn structural_commit_updates_external_watch_roots_and_reload_recovers_writes() {
        let root = std::env::temp_dir().join(format!("coflow-watch-structure-{}", SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
        let project = root.join("project");
        let external = root.join("external");
        std::fs::create_dir_all(project.join("data")).unwrap();
        std::fs::create_dir_all(&external).unwrap();
        std::fs::write(project.join("schema.cft"), "table Item { value: int; }").unwrap();
        std::fs::write(project.join("coflow.yaml"), "schema: schema.cft\ndata: data/\ncodegen:\n  - language: csharp\n    dir: generated/\n").unwrap();
        let host = EditorHost::new(Arc::new(NoopEditorEventSink)).unwrap();
        let id = host.load_project(&project.join("coflow.yaml")).unwrap().session_id;
        host.add_project_input(id, coflow_project::ProjectInputKind::Data, &external).unwrap();
        assert!(host.watchers.watches(id, &external));
        host.sessions.mark_needs_reload(id);
        assert!(host.create_project_file(id, coflow_project::ProjectInputKind::Data, &external, "blocked.cfd").is_err());
        assert!(!external.join("blocked.cfd").exists());
        host.reload_session(id).unwrap();
        host.create_project_file(id, coflow_project::ProjectInputKind::Data, &external, "allowed.cfd").unwrap();
        host.close_session(id).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

}
