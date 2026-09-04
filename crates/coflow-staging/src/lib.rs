//! Filesystem staging primitives used by mutation and release publication.
//!
//! A staged change is created beside its destination. Publication verifies the
//! destination against the caller's expectation, replaces it with a sibling
//! backup, and can restore that backup until [`StagedChange::finish`] is
//! called. Dropping an unfinished change restores the previous state and
//! removes staging residue.

#![forbid(unsafe_code)]

use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug)]
pub struct StagingError {
    path: PathBuf,
    kind: StagingErrorKind,
}

impl StagingError {
    #[must_use]
    pub fn is_conflict(&self) -> bool {
        matches!(self.kind, StagingErrorKind::Conflict)
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl fmt::Display for StagingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            StagingErrorKind::Conflict => write!(
                formatter,
                "file `{}` changed while its write was prepared",
                self.path.display()
            ),
            StagingErrorKind::Io(operation, error) => {
                write!(formatter, "failed to {operation} `{}`: {error}", self.path.display())
            }
        }
    }
}

impl std::error::Error for StagingError {}

#[derive(Debug)]
enum StagingErrorKind {
    Conflict,
    Io(&'static str, io::Error),
}

fn io_error(path: &Path, operation: &'static str, error: io::Error) -> StagingError {
    StagingError {
        path: path.to_path_buf(),
        kind: StagingErrorKind::Io(operation, error),
    }
}

fn conflict(path: &Path) -> StagingError {
    StagingError {
        path: path.to_path_buf(),
        kind: StagingErrorKind::Conflict,
    }
}

fn read_optional(path: &Path) -> Result<Option<Vec<u8>>, StagingError> {
    match fs::read(path) {
        Ok(contents) => Ok(Some(contents)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(io_error(path, "read", error)),
    }
}

pub trait StagedChange {
    /// Verifies the destination but does not modify it.
    fn verify(&self) -> Result<(), StagingError>;

    /// Publishes the staged change and stores enough state to restore it.
    fn publish(&mut self) -> Result<(), StagingError>;

    /// Restores the previous destination. Safe to call repeatedly.
    fn restore(&mut self);

    /// Marks publication permanent and removes the backup.
    fn finish(&mut self);
}

#[derive(Debug)]
pub struct StagedFile {
    path: PathBuf,
    expected: Option<Vec<u8>>,
    staging: PathBuf,
    backup: Option<PathBuf>,
    published: bool,
    finished: bool,
}

impl StagedFile {
    pub fn create(path: &Path, expected: Option<Vec<u8>>, contents: &[u8]) -> Result<Self, StagingError> {
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)
            .map_err(|error| io_error(parent, "create parent for", error))?;
        let staging = unique_sibling(path, "staging");
        let mut output = fs::File::create(&staging)
            .map_err(|error| io_error(&staging, "create staging file", error))?;
        if let Err(error) = output.write_all(contents) {
            let _ = fs::remove_file(&staging);
            return Err(io_error(&staging, "write staging file", error));
        }
        if let Err(error) = output.sync_all() {
            let _ = fs::remove_file(&staging);
            return Err(io_error(&staging, "sync staging file", error));
        }
        Ok(Self {
            path: path.to_path_buf(),
            expected,
            staging,
            backup: None,
            published: false,
            finished: false,
        })
    }
}

impl StagedChange for StagedFile {
    fn verify(&self) -> Result<(), StagingError> {
        if read_optional(&self.path)? == self.expected {
            Ok(())
        } else {
            Err(conflict(&self.path))
        }
    }

    fn publish(&mut self) -> Result<(), StagingError> {
        self.verify()?;
        replace_staged(
            &self.path,
            &self.staging,
            &mut self.backup,
            &mut self.published,
            false,
        )
    }

    fn restore(&mut self) {
        restore_staged(&self.path, self.published, &mut self.backup, false);
        self.published = false;
    }

    fn finish(&mut self) {
        self.finished = true;
        finish_staged(self.backup.take(), false);
    }
}

impl Drop for StagedFile {
    fn drop(&mut self) {
        if !self.finished {
            self.restore();
        }
        if self.staging.is_file() {
            let _ = fs::remove_file(&self.staging);
        }
    }
}

#[derive(Debug)]
pub struct StagedDirectory {
    path: PathBuf,
    staging: PathBuf,
    backup: Option<PathBuf>,
    published: bool,
    finished: bool,
}

impl StagedDirectory {
    /// Creates an empty sibling directory and returns a change that will
    /// replace `path` after the caller has populated the staging directory.
    pub fn create(path: &Path) -> Result<Self, StagingError> {
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)
            .map_err(|error| io_error(parent, "create parent for", error))?;
        let staging = unique_sibling(path, "staging");
        fs::create_dir(&staging)
            .map_err(|error| io_error(&staging, "create staging directory", error))?;
        Ok(Self {
            path: path.to_path_buf(),
            staging,
            backup: None,
            published: false,
            finished: false,
        })
    }

    #[must_use]
    pub fn staging(&self) -> &Path {
        &self.staging
    }
}

impl StagedChange for StagedDirectory {
    fn verify(&self) -> Result<(), StagingError> {
        if self.path.exists() && !self.path.is_dir() {
            return Err(io_error(
                &self.path,
                "replace non-directory",
                io::Error::new(io::ErrorKind::AlreadyExists, "not a directory"),
            ));
        }
        Ok(())
    }

    fn publish(&mut self) -> Result<(), StagingError> {
        self.verify()?;
        replace_staged(
            &self.path,
            &self.staging,
            &mut self.backup,
            &mut self.published,
            true,
        )
    }

    fn restore(&mut self) {
        restore_staged(&self.path, self.published, &mut self.backup, true);
        self.published = false;
    }

    fn finish(&mut self) {
        self.finished = true;
        finish_staged(self.backup.take(), true);
    }
}

impl Drop for StagedDirectory {
    fn drop(&mut self) {
        if !self.finished {
            self.restore();
        }
        if self.staging.is_dir() {
            let _ = fs::remove_dir_all(&self.staging);
        }
    }
}

#[derive(Debug)]
pub struct StagedRemoval {
    path: PathBuf,
    backup: Option<PathBuf>,
    removed: bool,
    finished: bool,
}

impl StagedRemoval {
    pub fn create(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
            backup: None,
            removed: false,
            finished: false,
        }
    }
}

impl StagedChange for StagedRemoval {
    fn verify(&self) -> Result<(), StagingError> {
        Ok(())
    }

    fn publish(&mut self) -> Result<(), StagingError> {
        if !self.path.exists() {
            return Ok(());
        }
        let backup = unique_sibling(&self.path, "backup");
        fs::rename(&self.path, &backup)
            .map_err(|error| io_error(&self.path, "back up file", error))?;
        self.backup = Some(backup);
        self.removed = true;
        Ok(())
    }

    fn restore(&mut self) {
        if self.removed {
            let _ = fs::remove_file(&self.path);
        }
        if let Some(backup) = self.backup.as_ref() {
            if fs::rename(backup, &self.path).is_ok() {
                self.backup = None;
            }
        }
        self.removed = false;
    }

    fn finish(&mut self) {
        self.finished = true;
        finish_staged(self.backup.take(), false);
    }
}

impl Drop for StagedRemoval {
    fn drop(&mut self) {
        if !self.finished {
            self.restore();
        }
    }
}

fn replace_staged(
    path: &Path,
    staging: &Path,
    backup: &mut Option<PathBuf>,
    published: &mut bool,
    directory: bool,
) -> Result<(), StagingError> {
    if path.exists() {
        let destination = unique_sibling(path, "backup");
        fs::rename(path, &destination).map_err(|error| io_error(path, "back up file", error))?;
        *backup = Some(destination);
    }
    if let Err(error) = fs::rename(staging, path) {
        restore_staged(path, *published, backup, directory);
        *published = false;
        return Err(io_error(path, "publish", error));
    }
    *published = true;
    Ok(())
}

fn restore_staged(
    path: &Path,
    published: bool,
    backup: &mut Option<PathBuf>,
    directory: bool,
) {
    if published {
        if directory {
            let _ = fs::remove_dir_all(path);
        } else {
            let _ = fs::remove_file(path);
        }
    }
    if let Some(backup_path) = backup.as_ref() {
        if fs::rename(backup_path, path).is_ok() {
            *backup = None;
        }
    }
}

fn finish_staged(backup: Option<PathBuf>, directory: bool) {
    if let Some(backup) = backup {
        if directory {
            let _ = fs::remove_dir_all(backup);
        } else {
            let _ = fs::remove_file(backup);
        }
    }
}

#[must_use]
pub fn unique_sibling(path: &Path, kind: &str) -> PathBuf {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("source");
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let revision = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    parent.join(format!(
        ".{name}.coflow-{kind}-{}-{timestamp}-{revision}",
        std::process::id()
    ))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::*;

    #[test]
    fn unfinished_staged_file_restores_previous_contents() {
        let root = tempfile::tempdir().expect("temp dir");
        let path = root.path().join("file.txt");
        fs::write(&path, b"old").expect("write old");

        let mut staged = StagedFile::create(&path, Some(b"old".to_vec()), b"new")
            .expect("stage file");
        staged.publish().expect("publish file");
        drop(staged);

        assert_eq!(fs::read(&path).expect("read restored"), b"old");
    }

    #[test]
    fn finished_staged_file_publishes_new_contents() {
        let root = tempfile::tempdir().expect("temp dir");
        let path = root.path().join("file.txt");
        fs::write(&path, b"old").expect("write old");

        let mut staged = StagedFile::create(&path, Some(b"old".to_vec()), b"new")
            .expect("stage file");
        staged.publish().expect("publish file");
        staged.finish();
        drop(staged);

        assert_eq!(fs::read(&path).expect("read published"), b"new");
    }

    #[test]
    fn staged_file_detects_conflicting_destination() {
        let root = tempfile::tempdir().expect("temp dir");
        let path = root.path().join("file.txt");
        fs::write(&path, b"old").expect("write old");

        let staged = StagedFile::create(&path, Some(b"expected".to_vec()), b"new")
            .expect("stage file");
        let error = staged.verify().expect_err("conflict should be detected");

        assert!(error.is_conflict());
    }

    #[test]
    fn unfinished_staged_directory_restores_previous_directory() {
        let root = tempfile::tempdir().expect("temp dir");
        let path = root.path().join("output");
        fs::create_dir(&path).expect("create old directory");
        fs::write(path.join("old.txt"), b"old").expect("write old file");

        let mut staged = StagedDirectory::create(&path).expect("stage directory");
        fs::write(staged.staging().join("new.txt"), b"new")
            .expect("write staged file");
        staged.publish().expect("publish directory");
        drop(staged);

        assert!(path.join("old.txt").is_file());
        assert!(!path.join("new.txt").exists());
    }
}
