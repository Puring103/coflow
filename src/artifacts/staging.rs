use super::diagnostic_set;
use coflow_api::{ArtifactContent, ArtifactSet, DiagnosticSet};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug)]
pub struct StagedArtifactDir {
    target: PathBuf,
    staging: PathBuf,
    committed: bool,
}

#[derive(Debug)]
pub struct StagedArtifactFile {
    target: PathBuf,
    staging: PathBuf,
    committed: bool,
}

pub(super) fn stage_artifact_set(
    dir: &Path,
    artifacts: ArtifactSet,
) -> Result<StagedArtifactDir, DiagnosticSet> {
    let staged = StagedArtifactDir::create(dir)?;
    for artifact in artifacts.files {
        let path = safe_artifact_path(staged.path(), &artifact.relative_path)
            .map_err(|err| diagnostic_set(dir, err))?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|err| {
                diagnostic_set(
                    dir,
                    format!("failed to create `{}`: {err}", parent.display()),
                )
            })?;
        }
        match artifact.content {
            ArtifactContent::Text(contents) => fs::write(&path, contents).map_err(|err| {
                diagnostic_set(
                    &path,
                    format!("failed to write `{}`: {err}", path.display()),
                )
            })?,
            ArtifactContent::Bytes(bytes) => fs::write(&path, bytes).map_err(|err| {
                diagnostic_set(
                    &path,
                    format!("failed to write `{}`: {err}", path.display()),
                )
            })?,
            ArtifactContent::Json(value) => {
                let file = fs::File::create(&path).map_err(|err| {
                    diagnostic_set(
                        &path,
                        format!("failed to create `{}`: {err}", path.display()),
                    )
                })?;
                serde_json::to_writer_pretty(file, &value).map_err(|err| {
                    diagnostic_set(
                        &path,
                        format!("failed to write `{}`: {err}", path.display()),
                    )
                })?;
            }
        }
    }
    Ok(staged)
}

pub fn commit_staged_dir_and_file(
    dir: StagedArtifactDir,
    file: Option<StagedArtifactFile>,
) -> Result<(), DiagnosticSet> {
    commit_staged_dirs_and_file(vec![dir], file)
}

pub fn commit_staged_dirs_and_file(
    mut dirs: Vec<StagedArtifactDir>,
    mut file: Option<StagedArtifactFile>,
) -> Result<(), DiagnosticSet> {
    let committed_file = if let Some(file) = file.as_mut() {
        let backup = replace_file_with_staging(&file.target, &file.staging)
            .map_err(|err| diagnostic_set(&file.target, err))?;
        file.committed = true;
        Some(CommittedFile {
            target: file.target.clone(),
            backup,
        })
    } else {
        None
    };

    let mut committed_dirs = Vec::new();
    for dir in &mut dirs {
        match replace_dir_with_staging(&dir.target, &dir.staging) {
            Ok(backup) => {
                dir.committed = true;
                committed_dirs.push(CommittedDir {
                    target: dir.target.clone(),
                    backup,
                });
            }
            Err(err) => {
                rollback_committed_dirs(&committed_dirs);
                if let Some(committed_file) = committed_file.as_ref() {
                    rollback_file_replace(&committed_file.target, committed_file.backup.as_deref());
                }
                return Err(diagnostic_set(&dir.target, err));
            }
        }
    }

    cleanup_committed_dirs(&committed_dirs).map_err(|err| {
        let path = committed_dirs
            .first()
            .map_or_else(PathBuf::new, |dir| dir.target.clone());
        diagnostic_set(path, err)
    })?;
    if let Some(committed_file) = committed_file {
        cleanup_committed_file(&committed_file)
            .map_err(|err| diagnostic_set(&committed_file.target, err))?;
    }
    Ok(())
}

fn safe_artifact_path(dir: &Path, relative_path: &Path) -> Result<PathBuf, String> {
    if relative_path.as_os_str().is_empty()
        || relative_path.is_absolute()
        || relative_path
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return Err("artifact path is empty".to_string());
    }
    Ok(dir.join(relative_path))
}

impl StagedArtifactDir {
    pub fn create(target: &Path) -> Result<Self, DiagnosticSet> {
        let parent = target.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent).map_err(|err| {
            diagnostic_set(
                target,
                format!("failed to create `{}`: {err}", parent.display()),
            )
        })?;
        let staging = unique_sidecar_path(target, "staging");
        if staging.exists() {
            fs::remove_dir_all(&staging).map_err(|err| {
                diagnostic_set(
                    target,
                    format!("failed to clean `{}`: {err}", staging.display()),
                )
            })?;
        }
        fs::create_dir(&staging).map_err(|err| {
            diagnostic_set(
                target,
                format!("failed to create `{}`: {err}", staging.display()),
            )
        })?;
        Ok(Self {
            target: target.to_path_buf(),
            staging,
            committed: false,
        })
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.staging
    }

    pub fn commit(mut self) -> Result<(), DiagnosticSet> {
        commit_staged_dir(&self.target, &self.staging)
            .map_err(|err| diagnostic_set(&self.target, err))?;
        self.committed = true;
        Ok(())
    }
}

impl Drop for StagedArtifactDir {
    fn drop(&mut self) {
        if !self.committed {
            let _ = fs::remove_dir_all(&self.staging);
        }
    }
}

impl StagedArtifactFile {
    pub(super) fn create_json<T: Serialize>(
        target: &Path,
        value: &T,
    ) -> Result<Self, DiagnosticSet> {
        let parent = target.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent).map_err(|err| {
            diagnostic_set(
                target,
                format!("failed to create `{}`: {err}", parent.display()),
            )
        })?;
        let staging = unique_sidecar_path(target, "staging");
        if staging.exists() {
            remove_any_path(&staging).map_err(|err| {
                diagnostic_set(
                    target,
                    format!("failed to clean `{}`: {err}", staging.display()),
                )
            })?;
        }
        let file = fs::File::create(&staging).map_err(|err| {
            diagnostic_set(
                target,
                format!("failed to create `{}`: {err}", staging.display()),
            )
        })?;
        serde_json::to_writer_pretty(file, value).map_err(|err| {
            diagnostic_set(
                target,
                format!("failed to write `{}`: {err}", staging.display()),
            )
        })?;
        Ok(Self {
            target: target.to_path_buf(),
            staging,
            committed: false,
        })
    }
}

impl Drop for StagedArtifactFile {
    fn drop(&mut self) {
        if !self.committed {
            let _ = fs::remove_file(&self.staging);
        }
    }
}

fn replace_file_with_staging(target: &Path, staging: &Path) -> Result<Option<PathBuf>, String> {
    if target.exists() && target.is_dir() {
        return Err(format!(
            "failed to replace `{}`: target is a directory",
            target.display()
        ));
    }
    let backup = if target.exists() {
        let backup = unique_sidecar_path(target, "backup");
        if backup.exists() {
            remove_any_path(&backup)
                .map_err(|err| format!("failed to clean `{}`: {err}", backup.display()))?;
        }
        fs::rename(target, &backup).map_err(|err| {
            format!(
                "failed to move old file `{}` to `{}`: {err}",
                target.display(),
                backup.display()
            )
        })?;
        Some(backup)
    } else {
        None
    };
    if let Err(err) = fs::rename(staging, target) {
        if let Some(backup) = backup.as_deref() {
            let _ = fs::rename(backup, target);
        }
        return Err(format!(
            "failed to move staged file `{}` to `{}`: {err}",
            staging.display(),
            target.display()
        ));
    }
    Ok(backup)
}

fn rollback_file_replace(target: &Path, backup: Option<&Path>) {
    let _ = fs::remove_file(target);
    if let Some(backup) = backup {
        let _ = fs::rename(backup, target);
    }
}

fn commit_staged_dir(target: &Path, staging: &Path) -> Result<(), String> {
    let backup = replace_dir_with_staging(target, staging)?;
    if let Some(backup) = backup {
        fs::remove_dir_all(&backup)
            .map_err(|err| format!("failed to remove `{}`: {err}", backup.display()))?;
    }
    Ok(())
}

fn replace_dir_with_staging(target: &Path, staging: &Path) -> Result<Option<PathBuf>, String> {
    if !target.exists() {
        fs::rename(staging, target).map_err(|err| {
            format!(
                "failed to move staged artifacts `{}` to `{}`: {err}",
                staging.display(),
                target.display()
            )
        })?;
        return Ok(None);
    }
    if !target.is_dir() {
        return Err(format!(
            "failed to replace output dir `{}`: target is not a directory",
            target.display()
        ));
    }
    let backup = unique_sidecar_path(target, "backup");
    if backup.exists() {
        remove_any_path(&backup)
            .map_err(|err| format!("failed to clean `{}`: {err}", backup.display()))?;
    }
    fs::rename(target, &backup).map_err(|err| {
        format!(
            "failed to move old output dir `{}` to `{}`: {err}",
            target.display(),
            backup.display()
        )
    })?;
    match fs::rename(staging, target) {
        Ok(()) => Ok(Some(backup)),
        Err(err) => {
            let _ = fs::rename(&backup, target);
            Err(format!(
                "failed to move staged artifacts `{}` to `{}`: {err}",
                staging.display(),
                target.display()
            ))
        }
    }
}

fn rollback_dir_replace(target: &Path, backup: Option<&Path>) {
    let _ = fs::remove_dir_all(target);
    if let Some(backup) = backup {
        let _ = fs::rename(backup, target);
    }
}

#[derive(Debug)]
struct CommittedFile {
    target: PathBuf,
    backup: Option<PathBuf>,
}

#[derive(Debug)]
struct CommittedDir {
    target: PathBuf,
    backup: Option<PathBuf>,
}

fn rollback_committed_dirs(committed_dirs: &[CommittedDir]) {
    for committed_dir in committed_dirs.iter().rev() {
        rollback_dir_replace(&committed_dir.target, committed_dir.backup.as_deref());
    }
}

fn cleanup_committed_dirs(committed_dirs: &[CommittedDir]) -> Result<(), String> {
    for committed_dir in committed_dirs {
        if let Some(backup) = committed_dir.backup.as_deref() {
            fs::remove_dir_all(backup)
                .map_err(|err| format!("failed to remove `{}`: {err}", backup.display()))?;
        }
    }
    Ok(())
}

fn cleanup_committed_file(committed_file: &CommittedFile) -> Result<(), String> {
    if let Some(backup) = committed_file.backup.as_deref() {
        fs::remove_file(backup)
            .map_err(|err| format!("failed to remove `{}`: {err}", backup.display()))?;
    }
    Ok(())
}

fn remove_any_path(path: &Path) -> std::io::Result<()> {
    if path.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

fn unique_sidecar_path(target: &Path, kind: &str) -> PathBuf {
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    let name = target
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("artifacts");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    parent.join(format!(
        ".{name}.coflow-{kind}-{}-{suffix}",
        std::process::id()
    ))
}
