//! 使用通道确定请求阶段，不依赖睡眠时间或线程调度。
#![allow(clippy::expect_used)]
use super::*;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    mpsc,
};
use std::time::Duration;

struct Fixture(StdPathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "coflow-language-concurrency-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(path.join("data")).unwrap();
        std::fs::write(path.join("coflow.yaml"), "schema: schema.cft\ndata: data/\ncodegen:\n  - language: csharp\n    dir: generated/\n").unwrap();
        std::fs::write(path.join("schema.cft"), "table Item { value: int; }\n").unwrap();
        std::fs::write(path.join("data/items.cfd"), "one: Item { value: 1 }\n").unwrap();
        Self(path)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn inflight_language_work_rejects_commit_and_does_not_hold_project_lock() {
    let fixture = Fixture::new();
    let store = SessionStore::new().unwrap();
    let id = store
        .load_project(&fixture.0.join("coflow.yaml"))
        .unwrap()
        .session_id;
    let entry = store.session(id).unwrap();
    let language = entry.state.read().language.clone();
    let (started, ready) = mpsc::channel();
    let (release, resume) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        language.run(|_| {
            started.send(()).unwrap();
            resume.recv_timeout(Duration::from_secs(10)).unwrap();
            Ok(())
        })
    });
    ready.recv_timeout(Duration::from_secs(10)).unwrap();
    let project_lock_available = entry.state.try_write().is_some();
    // 语言线程等待时发布失效通知，通知不得等待语言锁。
    entry
        .state
        .read()
        .language
        .invalidate(vec![fixture.0.join("data/items.cfd")]);
    release.send(()).unwrap();
    assert!(worker.join().unwrap().is_err());
    assert!(project_lock_available);
    assert!(entry.state.read().language.run(|_| Ok(())).is_ok());
}

#[test]
fn closing_session_rejects_inflight_language_and_prepared_reload() {
    let fixture = Fixture::new();
    let store = SessionStore::new().unwrap();
    let id = store
        .load_project(&fixture.0.join("coflow.yaml"))
        .unwrap()
        .session_id;
    let (entry, candidate) = store.build_reload_candidate(id).unwrap();
    let language = entry.state.read().language.clone();
    let (started, ready) = mpsc::channel();
    let (release, resume) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        language.run(|_| {
            started.send(()).unwrap();
            resume.recv_timeout(Duration::from_secs(10)).unwrap();
            Ok(())
        })
    });
    ready.recv_timeout(Duration::from_secs(10)).unwrap();
    store.close_session(id).unwrap();
    release.send(()).unwrap();
    assert!(worker.join().unwrap().is_err());
    assert!(SessionStore::commit_reload_candidate(id, &entry, candidate).is_err());
    assert!(store.session(id).is_err());
}

#[test]
fn committed_structure_error_is_distinct_from_a_failed_write() {
    let fixture = Fixture::new();
    let store = SessionStore::new().unwrap();
    let id = store
        .load_project(&fixture.0.join("coflow.yaml"))
        .unwrap()
        .session_id;
    let commit = coflow_project::create_project_file(
        &fixture.0.join("coflow.yaml"),
        coflow_project::ProjectInputKind::Data,
        &fixture.0.join("data"),
        "new.cfd",
    )
    .unwrap();
    store.close_session(id).unwrap();
    let error = store.reload_after_commit(id, commit).unwrap_err();
    assert!(matches!(
        error.kind,
        crate::editor::types::EditorErrorKind::Committed
    ));
    assert!(fixture.0.join("data/new.cfd").exists());
}
