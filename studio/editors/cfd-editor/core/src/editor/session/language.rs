//! 语言计算独立串行化；项目锁只保护项目状态，提交只记录待失效输入。
use crate::editor::types::EditorError;
use coflow_lsp::service::LanguageService;
use parking_lot::Mutex;
use std::{
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
};

#[derive(Default, Debug, Clone)]
struct Pending {
    project: Option<coflow_project::Project>,
    paths: Vec<PathBuf>,
}
#[derive(Debug)]
pub(crate) struct LanguageSession {
    service: Mutex<LanguageService>,
    pending: Mutex<Pending>,
    closed: AtomicBool,
}
impl LanguageSession {
    pub(crate) fn new(service: LanguageService) -> Self {
        Self {
            service: Mutex::new(service),
            pending: Mutex::new(Pending::default()),
            closed: AtomicBool::new(false),
        }
    }
    pub(crate) fn invalidate(&self, paths: Vec<PathBuf>) {
        self.pending.lock().paths.extend(paths);
    }
    pub(crate) fn rebase(&self, project: coflow_project::Project) {
        self.pending.lock().project = Some(project);
    }
    pub(crate) fn ensure_open(&self) -> Result<(), EditorError> {
        if self.closed.load(Ordering::Acquire) {
            Err(EditorError::not_found("session closed"))
        } else {
            Ok(())
        }
    }
    pub(crate) fn close(&self) {
        self.closed.store(true, Ordering::Release);
    }
    pub(crate) fn run<T>(
        &self,
        work: impl FnOnce(&mut LanguageService) -> Result<T, String>,
    ) -> Result<T, EditorError> {
        let mut service = self.service.lock();
        self.ensure_open()?;
        let pending = std::mem::take(&mut *self.pending.lock());
        let mut apply = || -> Result<(), String> {
            if let Some(project) = pending.project.clone() {
                service.rebase(project)?;
            }
            service.invalidate_files(&pending.paths)
        };
        if let Err(error) = apply() {
            // 应用失败时保留失效输入；期间到达的新基线优先于旧基线。
            let mut queued = self.pending.lock();
            if queued.project.is_none() {
                queued.project = pending.project;
            }
            queued.paths.extend(pending.paths);
            return Err(EditorError::other(error));
        }
        let result = work(&mut service).map_err(EditorError::other)?;
        self.ensure_open()?;
        // 项目在计算期间提交时，拒绝返回旧基线的结果；下一请求消费失效队列。
        let pending = self.pending.lock();
        if pending.project.is_some() || !pending.paths.is_empty() {
            return Err(EditorError::other(
                "project changed during language request",
            ));
        }
        Ok(result)
    }
}
