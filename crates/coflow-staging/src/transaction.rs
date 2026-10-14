//! 多文件发布与可观测恢复结果。
use crate::{StagedChange, StagedFile, StagedRemoval};
use std::{fs, io, path::PathBuf};

#[derive(Debug, Clone)]
pub struct FileChange {
    pub path: PathBuf,
    pub before: Option<Vec<u8>>,
    pub after: Option<Vec<u8>>,
}

#[derive(Debug)]
pub struct PublishFailure {
    pub message: String,
    pub conflict: bool,
    pub written: Vec<PathBuf>,
    pub restored: Vec<PathBuf>,
    pub restoration_failed: Vec<PathBuf>,
    pub restoration_conflicts: Vec<PathBuf>,
    pub restoration_errors: Vec<(PathBuf, String)>,
}

fn contents(path: &std::path::Path) -> io::Result<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(v) => Ok(Some(v)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

pub fn publish(changes: &[FileChange]) -> Result<Vec<PathBuf>, PublishFailure> {
    let mut staged: Vec<Box<dyn StagedChange>> = Vec::new();
    for change in changes {
        match contents(&change.path) {
            Ok(bytes) if bytes == change.before => {}
            Ok(_) => {
                return Err(PublishFailure {
                    message: format!("file changed: {}", change.path.display()),
                    conflict: true,
                    written: Vec::new(),
                    restored: Vec::new(),
                    restoration_failed: Vec::new(),
                    restoration_conflicts: Vec::new(),
                    restoration_errors: Vec::new(),
                })
            }
            Err(e) => {
                return Err(PublishFailure {
                    message: e.to_string(),
                    conflict: false,
                    written: Vec::new(),
                    restored: Vec::new(),
                    restoration_failed: Vec::new(),
                    restoration_conflicts: Vec::new(),
                    restoration_errors: Vec::new(),
                })
            }
        }
        let stage: Box<dyn StagedChange> = if let Some(after) = &change.after {
            Box::new(
                StagedFile::create(&change.path, change.before.clone(), after).map_err(|e| {
                    PublishFailure {
                        message: e.to_string(),
                        conflict: e.is_conflict(),
                        written: Vec::new(),
                        restored: Vec::new(),
                        restoration_failed: Vec::new(),
                        restoration_conflicts: Vec::new(),
                        restoration_errors: Vec::new(),
                    }
                })?,
            )
        } else {
            Box::new(StagedRemoval::create(&change.path))
        };
        staged.push(stage);
    }
    publish_prepared(changes, staged)
}

fn publish_prepared(
    changes: &[FileChange],
    staged: Vec<Box<dyn StagedChange>>,
) -> Result<Vec<PathBuf>, PublishFailure> {
    let paths = changes
        .iter()
        .map(|change| change.path.clone())
        .collect::<Vec<_>>();
    publish_with(&paths, staged, |index| {
        match contents(&changes[index].path) {
            Ok(current) if current == changes[index].before => Ok(()),
            Ok(_) => Err((
                true,
                format!("file changed: {}", changes[index].path.display()),
            )),
            Err(error) => Err((false, error.to_string())),
        }
    })
}

/// 文件与生成目录共用发布和恢复编排，失败报告保留每个实际处理的目标。
pub fn publish_staged(
    changes: Vec<(PathBuf, Box<dyn StagedChange>)>,
) -> Result<Vec<PathBuf>, PublishFailure> {
    let (paths, staged): (Vec<_>, Vec<_>) = changes.into_iter().unzip();
    publish_with(&paths, staged, |_| Ok(()))
}

fn publish_with(
    paths: &[PathBuf],
    mut staged: Vec<Box<dyn StagedChange>>,
    mut verify: impl FnMut(usize) -> Result<(), (bool, String)>,
) -> Result<Vec<PathBuf>, PublishFailure> {
    let mut written = Vec::new();
    // 全部准备完成后先验证所有目标，再执行逐目标验证与发布。
    for (index, stage) in staged.iter().enumerate() {
        let checked = verify(index).and_then(|()| {
            stage
                .verify()
                .map_err(|error| (error.is_conflict(), error.to_string()))
        });
        if let Err((conflict, message)) = checked {
            return Err(PublishFailure {
                message,
                conflict,
                written,
                restored: Vec::new(),
                restoration_failed: Vec::new(),
                restoration_conflicts: Vec::new(),
                restoration_errors: Vec::new(),
            });
        }
    }
    for index in 0..staged.len() {
        let result = verify(index).and_then(|()| {
            staged[index]
                .publish()
                .map_err(|error| (error.is_conflict(), error.to_string()))
        });
        if let Err((conflict, message)) = result {
            let mut restored = Vec::new();
            let mut restoration_failed = Vec::new();
            let mut restoration_conflicts = Vec::new();
            let mut restoration_errors = Vec::new();
            for (stage, path) in staged[..=index].iter_mut().zip(&paths[..=index]).rev() {
                match stage.restore() {
                    Ok(()) => {
                        if written.contains(path) {
                            restored.push(path.clone());
                        }
                    }
                    Err(error) => {
                        restoration_failed.push(path.clone());
                        if error.is_conflict() {
                            restoration_conflicts.push(path.clone());
                        }
                        restoration_errors.push((path.clone(), error.to_string()));
                    }
                }
            }
            return Err(PublishFailure {
                message,
                conflict: conflict || !restoration_conflicts.is_empty(),
                written,
                restored,
                restoration_failed,
                restoration_conflicts,
                restoration_errors,
            });
        }
        written.push(paths[index].clone());
    }
    for stage in &mut staged {
        stage.finish();
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::StagingError;

    struct FailingPublication {
        path: PathBuf,
        external_change: Option<PathBuf>,
    }
    impl StagedChange for FailingPublication {
        fn verify(&self) -> Result<(), StagingError> {
            Ok(())
        }
        fn publish(&mut self) -> Result<(), StagingError> {
            if let Some(path) = &self.external_change {
                fs::write(path, b"external").unwrap();
            }
            Err(crate::io_error(
                &self.path,
                "publish",
                io::Error::other("injected publication failure"),
            ))
        }
        fn restore(&mut self) -> Result<(), StagingError> {
            Ok(())
        }
        fn finish(&mut self) {}
    }

    fn fail_second_publication(external_change: bool) -> (tempfile::TempDir, PublishFailure) {
        let root = tempfile::tempdir().unwrap();
        let first = root.path().join("first.cfd");
        let second = root.path().join("second.cfd");
        fs::write(&first, b"old").unwrap();
        fs::write(&second, b"old").unwrap();
        let changes: Vec<_> = [&first, &second]
            .into_iter()
            .map(|path| FileChange {
                path: path.clone(),
                before: Some(b"old".to_vec()),
                after: Some(b"new".to_vec()),
            })
            .collect();
        let staged: Vec<Box<dyn StagedChange>> = vec![
            Box::new(StagedFile::create(&first, Some(b"old".to_vec()), b"new").unwrap()),
            Box::new(FailingPublication {
                path: second,
                external_change: external_change.then_some(first),
            }),
        ];
        let failure = publish_prepared(&changes, staged).unwrap_err();
        (root, failure)
    }

    #[test]
    fn partial_publication_restores_written_files_and_reports_them() {
        let (root, failure) = fail_second_publication(false);
        let first = root.path().join("first.cfd");
        assert!(!failure.conflict);
        assert_eq!(failure.written, vec![first.clone()]);
        assert_eq!(failure.restored, vec![first.clone()]);
        assert!(failure.restoration_failed.is_empty());
        assert_eq!(fs::read(first).unwrap(), b"old");
        assert_eq!(fs::read(root.path().join("second.cfd")).unwrap(), b"old");
    }

    #[test]
    fn restoration_conflict_preserves_external_changes_and_reports_the_failure() {
        let (root, failure) = fail_second_publication(true);
        let first = root.path().join("first.cfd");
        assert!(failure.conflict);
        assert_eq!(failure.written, vec![first.clone()]);
        assert!(failure.restored.is_empty());
        assert_eq!(failure.restoration_failed, vec![first.clone()]);
        assert_eq!(failure.restoration_conflicts, vec![first.clone()]);
        assert_eq!(fs::read(first).unwrap(), b"external");
        assert_eq!(fs::read(root.path().join("second.cfd")).unwrap(), b"old");
    }
}
