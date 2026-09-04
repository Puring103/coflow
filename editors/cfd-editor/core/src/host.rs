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
        self.sessions.reload_session(session_id)
    }

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
}
