//! 多文件发布与可观测恢复结果。
use std::{fs, io, path::PathBuf};
use crate::{StagedChange, StagedFile, StagedRemoval};

#[derive(Debug, Clone)]
pub struct FileChange { pub path: PathBuf, pub before: Option<Vec<u8>>, pub after: Option<Vec<u8>> }

#[derive(Debug)]
pub struct PublishFailure {
    pub message: String,
    pub conflict: bool,
    pub written: Vec<PathBuf>,
    pub restoration_failed: Vec<PathBuf>,
}

fn contents(path: &std::path::Path) -> io::Result<Option<Vec<u8>>> {
    match fs::read(path) { Ok(v) => Ok(Some(v)), Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None), Err(e) => Err(e) }
}

pub fn publish(changes: &[FileChange]) -> Result<Vec<PathBuf>, PublishFailure> {
    let mut staged: Vec<Box<dyn StagedChange>> = Vec::new();
    for change in changes {
        match contents(&change.path) {
            Ok(bytes) if bytes == change.before => {},
            Ok(_) => return Err(PublishFailure { message: format!("file changed: {}", change.path.display()), conflict: true, written: Vec::new(), restoration_failed: Vec::new() }),
            Err(e) => return Err(PublishFailure { message: e.to_string(), conflict: false, written: Vec::new(), restoration_failed: Vec::new() }),
        }
        let stage: Box<dyn StagedChange> = if let Some(after) = &change.after {
            Box::new(StagedFile::create(&change.path, change.before.clone(), after).map_err(|e| PublishFailure {
                message: e.to_string(), conflict: e.is_conflict(), written: Vec::new(), restoration_failed: Vec::new()
            })?)
        } else { Box::new(StagedRemoval::create(&change.path)) };
        staged.push(stage);
    }
    let mut written = Vec::new();
    for index in 0..staged.len() {
        // 删除与替换采用同样的内容前置条件；不把不存在的路径视为删除成功。
        let result = match contents(&changes[index].path) {
            Ok(current) if current == changes[index].before => staged[index].publish().map_err(|e| (e.is_conflict(), e.to_string())),
            Ok(_) => Err((true, format!("file changed: {}", changes[index].path.display()))),
            Err(e) => Err((false, e.to_string())),
        };
        if let Err((conflict, message)) = result {
            for stage in staged.iter_mut().rev() { stage.restore(); }
            let restoration_failed: Vec<_> = changes.iter().take(index + 1).filter(|c| contents(&c.path).ok().as_ref() != Some(&c.before)).map(|c| c.path.clone()).collect();
            return Err(PublishFailure { message, conflict, written, restoration_failed });
        }
        written.push(changes[index].path.clone());
    }
    for stage in &mut staged { stage.finish(); }
    Ok(written)
}
