//! 项目源码快照：磁盘基线和未保存覆盖相互独立，解析结果由所有宿主共享。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use coflow_language::cfd::{parse_cfd, CfdAst, CfdSyntaxDiagnostic};

#[derive(Debug)]
pub struct CfdSourceSnapshot {
    pub text: Arc<str>,
    pub syntax: CfdAst,
    pub errors: Vec<CfdSyntaxDiagnostic>,
}

impl CfdSourceSnapshot {
    fn parse(text: Arc<str>) -> Self {
        let (syntax, errors) = parse_cfd(&text);
        Self {
            text,
            syntax,
            errors,
        }
    }
}

#[derive(Debug, Default)]
struct SourceVersions {
    disk: Option<Arc<CfdSourceSnapshot>>,
    overlay: Option<Arc<CfdSourceSnapshot>>,
}

#[derive(Debug, Default)]
pub struct CfdSourceStore {
    sources: Mutex<BTreeMap<PathBuf, SourceVersions>>,
    pub(crate) paths: Mutex<Option<Arc<[PathBuf]>>>,
}

impl CfdSourceStore {
    /// 显式读取磁盘；内容相同时复用已有 AST，不依赖时间戳判断相等。
    pub fn read(&self, path: &Path) -> std::io::Result<Arc<CfdSourceSnapshot>> {
        let mut sources = self
            .sources
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let text = std::fs::read_to_string(path)?;
        let versions = sources.entry(crate::normalize_path(path)).or_default();
        let snapshot = Self::matching(versions, &text)
            .unwrap_or_else(|| Arc::new(CfdSourceSnapshot::parse(text.into())));
        versions.disk = Some(Arc::clone(&snapshot));
        Ok(snapshot)
    }

    /// 文档输入期间复用磁盘基线；文件事件到达时由宿主显式失效。
    pub fn cached_or_read(&self, path: &Path) -> std::io::Result<Arc<CfdSourceSnapshot>> {
        let cached = self
            .sources
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&crate::normalize_path(path))
            .and_then(|versions| versions.disk.clone());
        cached.map_or_else(|| self.read(path), Ok)
    }

    /// 覆盖只影响调用者拿到的快照，绝不修改磁盘基线。
    pub fn overlay(&self, path: &Path, text: Arc<str>) -> Arc<CfdSourceSnapshot> {
        let mut sources = self
            .sources
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let versions = sources.entry(crate::normalize_path(path)).or_default();
        let snapshot = Self::matching(versions, &text)
            .unwrap_or_else(|| Arc::new(CfdSourceSnapshot::parse(text)));
        versions.overlay = Some(Arc::clone(&snapshot));
        snapshot
    }

    pub fn invalidate(&self, path: &Path) {
        *self
            .paths
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        if let Some(versions) = self
            .sources
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get_mut(&crate::normalize_path(path))
        {
            versions.disk = None;
        }
    }

    pub fn clear(&self) {
        *self
            .paths
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        self.sources
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
    }

    fn matching(versions: &SourceVersions, text: &str) -> Option<Arc<CfdSourceSnapshot>> {
        versions
            .disk
            .iter()
            .chain(versions.overlay.iter())
            .find(|snapshot| snapshot.text.as_ref() == text)
            .cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlays_share_parses_without_replacing_disk_and_invalidation_observes_changes() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("items.cfd");
        std::fs::write(&path, "a: Item {}").unwrap();
        let store = CfdSourceStore::default();
        let disk = store.read(&path).unwrap();
        assert!(Arc::ptr_eq(
            &disk,
            &store.overlay(&path, Arc::clone(&disk.text))
        ));
        let overlay = store.overlay(&path, "b: Item {}".into());
        assert!(!Arc::ptr_eq(&disk, &overlay));
        assert!(Arc::ptr_eq(&disk, &store.cached_or_read(&path).unwrap()));
        std::fs::write(&path, overlay.text.as_bytes()).unwrap();
        store.invalidate(&path);
        assert!(Arc::ptr_eq(&overlay, &store.cached_or_read(&path).unwrap()));
    }
}
