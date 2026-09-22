//! Session state and host-facing editor operations.
//!
//! `SessionStore` owns a small population of `EditorSession`s — one per
//! loaded project — and dispatches editor commands through project sessions.
//! Each session is wrapped in its own `RwLock` so reads
//! don't block one another and a write is scoped to a single session.
//!
//! 模块职责：
//! - `mod.rs`：会话类型 + 生命周期/重载/路径上下文；
//! - `settings_commands.rs`：编辑器展示设置写方法；
//! - `project_commands.rs`：check/build/diff 与项目结构变更；
//! - `operations/`：数据查询/mutation/类型化语言文档同步；
//! - `row_build.rs`：行快照与排序辅助；
//! - `mutation_apply.rs`：字段写回与集合编辑；
//! - `errors.rs`：诊断到 `EditorError` 的统一映射。

mod build;
mod diagnostics;
mod dimension;
pub(crate) mod errors;
mod graph;
mod language;
pub(crate) mod mutation_apply;
mod operations;
mod project_commands;
mod publication;
mod revision;
pub(crate) mod row_build;
mod settings_commands;

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::Path as StdPath;
use std::path::PathBuf as StdPathBuf;
use std::sync::Arc;

use parking_lot::RwLock;

use coflow_project::ProjectQueries;

use crate::editor::types::{EditorError, ProjectBootstrap};

pub use diagnostics::Diagnostics;
// 跨模块复用：行构建/mutation/错误映射统一经 `session::` 路径暴露，
// 子模块不再经 `super::*` 通配导入，避免分层被打破。
use build::SessionSnapshotParts;
pub(crate) use build::{build_session, diagnostic_messages};
use revision::{RevisionCoordinator, RevisionTicket};
pub(crate) use row_build::project_bootstrap;

/// A loaded project. Held inside `Arc<RwLock<…>>` so multi-session and
/// multi-reader access stay independent.
pub struct EditorSession {
    pub project_root: std::path::PathBuf,
    /// Path to the project's `coflow.yaml` used by project actions and reloads.
    pub yaml_path: std::path::PathBuf,
    pub project_session: coflow_project::WriteProjectSession,
    pub(crate) schema_revision: u32,
    pub diagnostics: Diagnostics,
    pub(crate) language: Arc<language::LanguageSession>,
    pub(crate) schema_files: HashSet<String>,
    pub(crate) schema_type_names: Vec<String>,
    pub(crate) file_type_counts: BTreeMap<String, BTreeMap<String, usize>>,
    pub(crate) ref_target_cache: HashMap<String, Vec<crate::editor::types::RefTarget>>,
    pub(crate) shape_cache: crate::editor::convert::ShapeCache,
    revisions: RevisionCoordinator,
}

impl std::fmt::Debug for EditorSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EditorSession")
            .field("project_root", &self.project_root)
            .field("source_files", &self.queries().source_file_count())
            .field("records", &self.queries().record_count())
            .finish_non_exhaustive()
    }
}

impl EditorSession {
    /// 提交前检查生命周期与版本空间，保证落盘后的发布不会失败。
    pub(crate) fn ensure_writable(&self) -> Result<(), EditorError> {
        self.language.ensure_open()?;
        if self.revisions.current() == u32::MAX || self.schema_revision == u32::MAX {
            return Err(EditorError::session(
                "session revision exhausted; reopen project",
            ));
        }
        Ok(())
    }

    pub(crate) fn queries(&self) -> ProjectQueries<'_> {
        self.project_session.queries()
    }

    pub(crate) fn commit_internal_write(&mut self, paths: &[String]) {
        self.revisions
            .commit_internal_write(&self.project_root, paths);
    }
}

#[derive(Debug)]
pub(crate) struct SessionEntry {
    pub(crate) state: RwLock<EditorSession>,
}

struct ReloadCandidate {
    base_revision: RevisionTicket,
    session: EditorSession,
    snapshot: SessionSnapshotParts,
}

#[derive(Debug)]
struct Inner {
    next_id: u32,
    sessions: HashMap<u32, Arc<SessionEntry>>,
}

/// 列统计中间态，仅 `row_build` 内部使用。
#[derive(Default)]
pub(crate) struct ColumnStats {
    pub(crate) type_names: BTreeSet<String>,
    pub(crate) max_summary_len: usize,
}

#[derive(Debug)]
pub struct SessionStore {
    inner: RwLock<Inner>,
}

impl SessionStore {
    pub fn new() -> Result<Self, EditorError> {
        Ok(Self {
            inner: RwLock::new(Inner {
                next_id: 0,
                sessions: HashMap::new(),
            }),
        })
    }

    pub fn init_project(&self, dir: &StdPath) -> Result<ProjectBootstrap, EditorError> {
        let outcome = coflow_project::init_project(dir)
            .map_err(|err| EditorError::project(diagnostic_messages(&err)))?;
        self.load_project(&outcome.config_path)
    }

    pub fn load_project(&self, yaml_path: &StdPath) -> Result<ProjectBootstrap, EditorError> {
        let (session, snapshot_partial) = build_session(yaml_path)?;
        let mut inner = self.inner.write();
        inner.next_id = inner
            .next_id
            .checked_add(1)
            .ok_or_else(|| EditorError::session("session id exhausted"))?;
        let id = inner.next_id;
        let bootstrap = project_bootstrap(id, &session, snapshot_partial);
        inner.sessions.insert(
            id,
            Arc::new(SessionEntry {
                state: RwLock::new(session),
            }),
        );
        drop(inner);
        Ok(bootstrap)
    }

    pub fn project_root_for(&self, id: u32) -> Result<StdPathBuf, EditorError> {
        let entry = self.session(id)?;
        let root = entry.state.read().project_root.clone();
        Ok(root)
    }

    pub fn source_file_path(&self, id: u32, file_path: &str) -> Result<StdPathBuf, EditorError> {
        let entry = self.session(id)?;
        let session = entry.state.read();
        if !session.queries().has_source_file(file_path)
            && !session.schema_files.contains(file_path)
        {
            return Err(EditorError::not_found(format!(
                "`{file_path}` is not a source file in the current project"
            )));
        }
        let project_root = session.project_root.clone();
        drop(session);
        let path =
            coflow_project::canonicalize_path(project_root.join(file_path)).map_err(|error| {
                EditorError::not_found(format!("failed to resolve `{file_path}`: {error}"))
            })?;
        if !path.is_file() {
            return Err(EditorError::not_found(format!(
                "source file `{file_path}` does not exist"
            )));
        }
        Ok(path)
    }

    pub(crate) fn project_action_context(&self, id: u32) -> Result<StdPathBuf, EditorError> {
        let entry = self.session(id)?;
        let yaml_path = entry.state.read().yaml_path.clone();
        Ok(yaml_path)
    }

    pub fn reload_session(&self, id: u32) -> Result<ProjectBootstrap, EditorError> {
        for _ in 0..3 {
            let (entry, candidate) = self.build_reload_candidate(id)?;
            if let Some(snapshot) = Self::commit_reload_candidate(id, &entry, candidate)? {
                return Ok(snapshot);
            }
        }
        Err(EditorError::session("project kept changing during reload"))
    }

    fn build_reload_candidate(
        &self,
        id: u32,
    ) -> Result<(Arc<SessionEntry>, ReloadCandidate), EditorError> {
        let entry = self.session(id)?;
        let (yaml_path, base_revision) = {
            let session = entry.state.read();
            (session.yaml_path.clone(), session.revisions.begin_reload())
        };
        let (session, snapshot) = build_session(&yaml_path)?;
        Ok((
            entry,
            ReloadCandidate {
                base_revision,
                session,
                snapshot,
            },
        ))
    }

    fn commit_reload_candidate(
        id: u32,
        entry: &SessionEntry,
        mut candidate: ReloadCandidate,
    ) -> Result<Option<ProjectBootstrap>, EditorError> {
        let mut state = entry.state.write();
        state.ensure_writable()?;
        let Some(revisions) = state.revisions.commit_reload(candidate.base_revision) else {
            return Ok(None);
        };
        candidate.session.language = Arc::clone(&state.language);
        candidate
            .session
            .language
            .rebase(candidate.session.project_session.project().clone());
        candidate.session.revisions = revisions;
        candidate.session.schema_revision = state.schema_revision.saturating_add(1);
        let bootstrap = project_bootstrap(id, &candidate.session, candidate.snapshot);
        *state = candidate.session;
        drop(state);
        Ok(Some(bootstrap))
    }

    pub(crate) fn has_external_file_changes(
        &self,
        id: u32,
        paths: &[std::path::PathBuf],
    ) -> Result<bool, EditorError> {
        let entry = self.session(id)?;
        let session = entry.state.read();
        Ok(session
            .revisions
            .has_external_change(&session.project_root, paths))
    }

    pub fn close_session(&self, id: u32) -> Result<(), EditorError> {
        if let Some(entry) = self.inner.write().sessions.remove(&id) {
            entry.state.read().language.close();
        }
        Ok(())
    }

    pub(crate) fn session(&self, id: u32) -> Result<Arc<SessionEntry>, EditorError> {
        let inner = self.inner.read();
        inner
            .sessions
            .get(&id)
            .cloned()
            .ok_or_else(|| EditorError::not_found(format!("unknown session id {id}")))
    }
}

#[cfg(test)]
mod concurrency_tests;
