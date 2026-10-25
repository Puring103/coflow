//! CFD 文件刷新复用编译后的 schema，并在会话锁外构建下一代数据。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use coflow_runtime::{ProjectRefreshContext, WriteProjectSession};

use super::build::{type_navigation, FileTypeCounts, FileTypeNames, SessionSnapshotParts};
use super::diagnostics::diagnostics_from_store;
use super::revision::RevisionTicket;
use super::{
    diagnostic_messages, project_bootstrap, Diagnostics, EditorSession, SessionEntry, SessionStore,
};
use crate::editor::types::{EditorError, EditorErrorKind, ProjectBootstrap};

struct DataReloadInput {
    base_revision: RevisionTicket,
    context: ProjectRefreshContext,
    project_root: PathBuf,
    changed_paths: Vec<PathBuf>,
}

struct DataReloadCandidate {
    base_revision: RevisionTicket,
    engine: WriteProjectSession,
    diagnostics: Diagnostics,
    file_type_names: FileTypeNames,
    file_type_counts: FileTypeCounts,
    snapshot: SessionSnapshotParts,
}

impl DataReloadInput {
    fn build(self) -> Result<DataReloadCandidate, EditorError> {
        let engine = self
            .context
            .refresh_data_sources(&self.changed_paths)
            .map_err(|diagnostics| EditorError::project(diagnostic_messages(&diagnostics)))?;
        let queries = engine.queries();
        let snapshot = SessionSnapshotParts {
            file_tree: queries.file_tree(),
        };
        let (file_type_names, file_type_counts, _) = type_navigation(queries);
        let diagnostics = diagnostics_from_store(queries, &self.project_root);
        Ok(DataReloadCandidate {
            base_revision: self.base_revision,
            engine,
            diagnostics,
            file_type_names,
            file_type_counts,
            snapshot,
        })
    }
}

impl SessionStore {
    pub(crate) fn reload_session_paths(
        &self,
        id: u32,
        changed_paths: &[PathBuf],
    ) -> Result<ProjectBootstrap, EditorError> {
        let entry = self.session(id)?;
        loop {
            let input = {
                let state = entry.state.read();
                data_reload_input(&state, changed_paths)
            };
            let Some(input) = input else {
                return self.reload_session(id);
            };
            let paths = input.changed_paths.clone();
            let candidate = input.build()?;
            if let Some((snapshot, language)) =
                Self::commit_data_reload_candidate(id, &entry, candidate)
            {
                // LSP 保留未保存文档，磁盘变更只更新其项目基线；语言分析不占用会话写锁。
                language
                    .lock()
                    .refresh_data_files(&paths)
                    .map_err(|mut error| {
                        error.kind = EditorErrorKind::Committed;
                        error
                    })?;
                return Ok(snapshot);
            }
            // 构建期间有内部写入或其他刷新时，必须从最新代次重新取得缓存快照。
        }
    }

    fn commit_data_reload_candidate(
        id: u32,
        entry: &SessionEntry,
        candidate: DataReloadCandidate,
    ) -> Option<(
        ProjectBootstrap,
        Arc<parking_lot::Mutex<super::LanguageSession>>,
    )> {
        let mut state = entry.state.write();
        let revisions = state.revisions.commit_reload(candidate.base_revision)?;
        // 旧代次的索引可能很大，交换后留到锁外释放，避免析构阻塞并发读请求。
        let previous = (
            std::mem::replace(&mut state.engine, candidate.engine),
            std::mem::replace(&mut state.diagnostics, candidate.diagnostics),
            std::mem::replace(&mut state.file_type_names, candidate.file_type_names),
            std::mem::replace(&mut state.file_type_counts, candidate.file_type_counts),
            std::mem::take(&mut state.ref_target_cache),
        );
        state.revisions = revisions;
        // 引用目标依赖数据代次；schema 形状与语言草稿在纯 CFD 更新中继续复用。
        let bootstrap = project_bootstrap(id, &state, candidate.snapshot);
        let language = Arc::clone(&state.language);
        drop(state);
        drop(previous);
        Some((bootstrap, language))
    }
}

fn data_reload_input(session: &EditorSession, paths: &[PathBuf]) -> Option<DataReloadInput> {
    let mut changed_paths = Vec::with_capacity(paths.len());
    for path in paths {
        let path = if path.is_absolute() {
            path.clone()
        } else {
            session.project_root.join(path)
        };
        let path = coflow_runtime::normalize_path(&path);
        // 递归监听经常同时报告父目录，现存目录不应使普通文件修改退回完整重载。
        if path.is_dir() {
            continue;
        }
        if !has_data_extension(&path) {
            return None;
        }
        changed_paths.push(path);
    }
    if changed_paths.is_empty() {
        return None;
    }
    changed_paths.sort();
    changed_paths.dedup();
    Some(DataReloadInput {
        base_revision: session.revisions.begin_reload(),
        context: session.engine.refresh_context(),
        project_root: session.project_root.clone(),
        changed_paths,
    })
}

fn has_data_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("cfd"))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use coflow_runtime::CfdValue;
    use serde_json::json;
    use std::fs;

    fn project() -> (tempfile::TempDir, SessionStore, u32) {
        let root = tempfile::tempdir().expect("project directory");
        fs::create_dir(root.path().join("data")).expect("data directory");
        fs::write(root.path().join("coflow.yaml"),
            "schema: schema.cft\ndata: data/\ncodegen:\n  - language: csharp\n    dir: generated/\n")
            .expect("config");
        fs::write(
            root.path().join("schema.cft"),
            "table Item { value: int; }\n",
        )
        .expect("schema");
        fs::write(
            root.path().join("data/first.cfd"),
            "first: Item { value: 1 }\n",
        )
        .expect("first source");
        fs::write(
            root.path().join("data/second.cfd"),
            "second: Item { value: 2 }\n",
        )
        .expect("second source");
        let store = SessionStore::new().expect("store");
        let id = store
            .load_project(&root.path().join("coflow.yaml"))
            .expect("load")
            .session_id;
        (root, store, id)
    }

    fn value(entry: &SessionEntry, id: &str) -> CfdValue {
        entry
            .state
            .read()
            .queries()
            .record_view("Item", id)
            .expect("record")
            .record
            .fields()
            .get("value")
            .expect("value")
            .clone()
    }

    #[test]
    fn data_refresh_preserves_schema_shapes_and_open_language_drafts() {
        let (root, store, id) = project();
        let entry = store.session(id).expect("session");
        let (language, shape, revision) = {
            let state = entry.state.read();
            (
                Arc::clone(&state.language),
                state
                    .shape_cache
                    .get(state.queries(), "Item", "value")
                    .expect("shape"),
                state.revisions.current(),
            )
        };
        let draft = store
            .sync_language_document(id, "data/first.cfd", "first: Item { value: }\n", 1)
            .expect("draft");
        assert!(!draft.syntax_valid);
        let second = root.path().join("data/second.cfd");
        fs::write(&second, "second: Item { value: 22 }\n").expect("external update");
        let snapshot = store
            .reload_session_paths(id, &[root.path().join("data"), second])
            .expect("data refresh");
        assert!(snapshot.revision > revision);
        assert_eq!(value(&entry, "second"), CfdValue::Int(22));
        let state = entry.state.read();
        assert!(Arc::ptr_eq(&language, &state.language));
        assert!(Arc::ptr_eq(
            &shape,
            &state
                .shape_cache
                .get(state.queries(), "Item", "value")
                .expect("cached shape")
        ));
        assert_eq!(state.file_type_counts["data/second.cfd"]["Item"], 1);
        drop(state);
        let uri = coflow_lsp::EmbeddedLsp::file_uri(&root.path().join("data/first.cfd"));
        let mut language = language.lock();
        assert!(language.documents.contains(&uri));
        let (tokens, _) = language
            .server
            .request(
                "textDocument/semanticTokens/full",
                &json!({"textDocument": {"uri": uri}}),
            )
            .expect("draft tokens");
        drop(language);
        assert_eq!(
            tokens
                .get("x-coflow-syntax-valid")
                .and_then(serde_json::Value::as_bool),
            Some(false)
        );
    }

    #[test]
    fn stale_data_candidate_cannot_replace_a_newer_internal_write() {
        let (root, store, id) = project();
        let entry = store.session(id).expect("session");
        let path = root.path().join("data/first.cfd");
        fs::write(&path, "first: Item { value: 10 }\n").expect("candidate source");
        let candidate = data_reload_input(&entry.state.read(), std::slice::from_ref(&path))
            .expect("data input")
            .build()
            .expect("candidate");
        fs::write(&path, "first: Item { value: 99 }\n").expect("newer source");
        entry
            .state
            .write()
            .commit_internal_write(&[coflow_runtime::PublishedFile::from_content(
                path.clone(),
                Some(b"first: Item { value: 99 }\n"),
            )]);
        assert!(SessionStore::commit_data_reload_candidate(id, &entry, candidate).is_none());
        store
            .reload_session_paths(id, std::slice::from_ref(&path))
            .expect("latest refresh");
        assert_eq!(value(&entry, "first"), CfdValue::Int(99));
        assert!(!store
            .has_external_file_changes(id, &[path])
            .expect("internal fingerprint"));
    }

    #[test]
    fn data_file_creation_and_deletion_refresh_navigation() {
        let (root, store, id) = project();
        let entry = store.session(id).expect("session");
        let language = Arc::clone(&entry.state.read().language);
        let path = root.path().join("data/third.cfd");
        fs::write(&path, "third: Item { value: 3 }\n").expect("new source");
        store
            .reload_session_paths(id, std::slice::from_ref(&path))
            .expect("create refresh");
        assert_eq!(value(&entry, "third"), CfdValue::Int(3));
        assert_eq!(
            entry.state.read().file_type_counts["data/third.cfd"]["Item"],
            1
        );
        fs::remove_file(&path).expect("delete source");
        store
            .reload_session_paths(id, &[path])
            .expect("delete refresh");
        let state = entry.state.read();
        assert!(!state.queries().has_source_file("data/third.cfd"));
        assert!(!state.file_type_names.contains_key("data/third.cfd"));
        assert!(!state.file_type_counts.contains_key("data/third.cfd"));
        assert!(Arc::ptr_eq(&language, &state.language));
        drop(state);
    }

    #[test]
    fn schema_changes_build_a_new_schema_and_language_session() {
        let (root, store, id) = project();
        let entry = store.session(id).expect("session");
        let language = Arc::clone(&entry.state.read().language);
        let path = root.path().join("schema.cft");
        fs::write(&path, "table Item { value: int; added: int = 7; }\n").expect("new schema");
        store
            .reload_session_paths(id, &[path])
            .expect("schema refresh");
        let state = entry.state.read();
        assert!(!Arc::ptr_eq(&language, &state.language));
        assert!(state
            .shape_cache
            .get(state.queries(), "Item", "added")
            .is_some());
        drop(state);
    }

    #[test]
    fn source_save_reuses_data_session_and_keeps_invalid_source_editable() {
        let (root, store, id) = project();
        let entry = store.session(id).expect("session");
        let language = Arc::clone(&entry.state.read().language);
        let original = store
            .read_source_text(id, "data/first.cfd")
            .expect("read source");
        let invalid = "first: Item { value: }\n";
        let snapshot = store
            .write_source_text(id, "data/first.cfd", invalid, &original)
            .expect("save invalid CFD");
        assert!(snapshot
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == "error"));
        assert!(Arc::ptr_eq(&language, &entry.state.read().language));
        assert_eq!(
            store
                .read_source_text(id, "data/first.cfd")
                .expect("saved source"),
            invalid
        );
        assert!(!store
            .has_external_file_changes(id, &[root.path().join("data/first.cfd")])
            .expect("saved fingerprint"));
    }
}
