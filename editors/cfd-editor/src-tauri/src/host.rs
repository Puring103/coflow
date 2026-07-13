use std::path::{Path, PathBuf};
use std::sync::Arc;

use tauri::AppHandle;

use crate::editor::{EditorError, ProjectSnapshot, SessionStore};
use crate::watcher::ProjectWatchRegistry;

#[derive(Debug, Clone)]
pub(crate) struct EditorHost {
    sessions: Arc<SessionStore>,
    watchers: Arc<ProjectWatchRegistry>,
}

impl EditorHost {
    pub(crate) fn new() -> Result<Self, EditorError> {
        Ok(Self {
            sessions: Arc::new(SessionStore::new()?),
            watchers: Arc::new(ProjectWatchRegistry::default()),
        })
    }

    pub(crate) fn load_project(
        &self,
        app: AppHandle,
        yaml_path: &Path,
    ) -> Result<ProjectSnapshot, EditorError> {
        self.load_project_with_watch(yaml_path, |snapshot| {
            self.watchers.watch_session(app, snapshot)
        })
    }

    pub(crate) fn init_project(
        &self,
        app: AppHandle,
        dir: &Path,
    ) -> Result<ProjectSnapshot, EditorError> {
        self.init_project_with_watch(dir, |snapshot| {
            self.watchers.watch_session(app, snapshot)
        })
    }

    pub(crate) fn close_session(&self, session_id: u32) -> Result<(), EditorError> {
        self.sessions.close_session(session_id)?;
        self.watchers.unwatch_session(session_id);
        Ok(())
    }

    pub(crate) fn reload_session(&self, session_id: u32) -> Result<ProjectSnapshot, EditorError> {
        self.sessions.reload_session(session_id)
    }

    pub(crate) fn has_external_file_changes(
        &self,
        session_id: u32,
        changed_paths: &[PathBuf],
    ) -> Result<bool, EditorError> {
        self.sessions
            .has_external_file_changes(session_id, changed_paths)
    }

    pub(crate) fn sessions(&self) -> &SessionStore {
        &self.sessions
    }

    fn load_project_with_watch<F>(
        &self,
        yaml_path: &Path,
        start_watch: F,
    ) -> Result<ProjectSnapshot, EditorError>
    where
        F: FnOnce(&ProjectSnapshot) -> Result<(), EditorError>,
    {
        let snapshot = self.sessions.load_project(yaml_path)?;
        self.finish_open(snapshot, start_watch)
    }

    fn init_project_with_watch<F>(
        &self,
        dir: &Path,
        start_watch: F,
    ) -> Result<ProjectSnapshot, EditorError>
    where
        F: FnOnce(&ProjectSnapshot) -> Result<(), EditorError>,
    {
        let snapshot = self.sessions.init_project(dir)?;
        self.finish_open(snapshot, start_watch)
    }

    fn finish_open<F>(
        &self,
        snapshot: ProjectSnapshot,
        start_watch: F,
    ) -> Result<ProjectSnapshot, EditorError>
    where
        F: FnOnce(&ProjectSnapshot) -> Result<(), EditorError>,
    {
        if let Err(error) = start_watch(&snapshot) {
            self.sessions.close_session(snapshot.session_id)?;
            return Err(error);
        }
        Ok(snapshot)
    }
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::editor::EditorError;

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
        let host = EditorHost::new().expect("create editor host");

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
}
