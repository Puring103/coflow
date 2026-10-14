//! Filesystem staging primitives used by mutation and release publication.
//!
//! A staged change is created beside its destination. Publication verifies the
//! destination against the caller's expectation, replaces it with a sibling
//! backup, and can restore that backup until [`StagedChange::finish`] is
//! called. Dropping an unfinished change attempts restoration. External changes
//! remain in place, with the backup retained when restoration cannot complete.

#![forbid(unsafe_code)]

pub mod transaction;

use std::collections::BTreeMap;
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
    pub const fn is_conflict(&self) -> bool {
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
                write!(
                    formatter,
                    "failed to {operation} `{}`: {error}",
                    self.path.display()
                )
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
    ///
    /// # Errors
    ///
    /// Returns [`StagingError`] when the destination changed after preparation.
    fn verify(&self) -> Result<(), StagingError>;

    /// Publishes the staged change and stores enough state to restore it.
    ///
    /// # Errors
    ///
    /// Returns [`StagingError`] when verification or a filesystem operation fails.
    fn publish(&mut self) -> Result<(), StagingError>;

    /// Restores the previous destination without replacing external changes.
    ///
    /// # Errors
    ///
    /// Returns a conflict if the published destination changed, or an I/O error
    /// if restoration fails. The backup remains available when restoration fails.
    fn restore(&mut self) -> Result<(), StagingError>;

    /// Marks publication permanent and removes the backup.
    fn finish(&mut self);
}

#[derive(Debug)]
pub struct StagedFile {
    path: PathBuf,
    expected: Option<Vec<u8>>,
    contents: Vec<u8>,
    staging: PathBuf,
    backup: Option<PathBuf>,
    published: bool,
    finished: bool,
    restoration_attempted: bool,
}

impl StagedFile {
    /// Creates a sibling staging file containing `contents`.
    ///
    /// # Errors
    ///
    /// Returns [`StagingError`] when the staging file cannot be created, written, or synchronized.
    pub fn create(
        path: &Path,
        expected: Option<Vec<u8>>,
        contents: &[u8],
    ) -> Result<Self, StagingError> {
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent).map_err(|error| io_error(parent, "create parent for", error))?;
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
            contents: contents.to_vec(),
            staging,
            backup: None,
            published: false,
            finished: false,
            restoration_attempted: false,
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

    fn restore(&mut self) -> Result<(), StagingError> {
        self.restoration_attempted = true;
        // 回滚只移除本次发布的内容，发布后的外部修改必须保留。
        if self.published && read_optional(&self.path)?.as_deref() != Some(self.contents.as_slice())
        {
            return Err(conflict(&self.path));
        }
        restore_staged(&self.path, &mut self.published, &mut self.backup, false)?;
        self.published = false;
        Ok(())
    }

    fn finish(&mut self) {
        self.finished = true;
        finish_staged(self.backup.take(), false);
    }
}

impl Drop for StagedFile {
    fn drop(&mut self) {
        if !self.finished && !self.restoration_attempted {
            let _ = self.restore();
        }
        if self.staging.is_file() {
            let _ = fs::remove_file(&self.staging);
        }
    }
}

#[derive(Debug)]
pub struct StagedDirectory {
    path: PathBuf,
    original: DirectorySnapshot,
    staging: PathBuf,
    backup: Option<PathBuf>,
    published: bool,
    finished: bool,
    restoration_attempted: bool,
    published_contents: Option<DirectoryContents>,
}

impl StagedDirectory {
    /// Creates an empty sibling directory and returns a change that will
    /// replace `path` after the caller has populated the staging directory.
    ///
    /// # Errors
    ///
    /// Returns [`StagingError`] when the staging directory cannot be created.
    pub fn create(path: &Path) -> Result<Self, StagingError> {
        Self::create_with_snapshot(path, DirectorySnapshot::capture(path)?)
    }

    /// Uses the read-only snapshot captured while preparing the output.
    pub fn create_with_snapshot(
        path: &Path,
        original: DirectorySnapshot,
    ) -> Result<Self, StagingError> {
        original.verify(path)?;
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent).map_err(|error| io_error(parent, "create parent for", error))?;
        let staging = unique_sibling(path, "staging");
        fs::create_dir(&staging)
            .map_err(|error| io_error(&staging, "create staging directory", error))?;
        Ok(Self {
            path: path.to_path_buf(),
            original,
            staging,
            backup: None,
            published: false,
            finished: false,
            restoration_attempted: false,
            published_contents: None,
        })
    }

    #[must_use]
    pub fn staging(&self) -> &Path {
        &self.staging
    }
}

impl StagedChange for StagedDirectory {
    fn verify(&self) -> Result<(), StagingError> {
        self.original.verify(&self.path)?;
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
        self.published_contents = Some(directory_contents(&self.staging)?);
        replace_staged(
            &self.path,
            &self.staging,
            &mut self.backup,
            &mut self.published,
            true,
        )
    }

    fn restore(&mut self) -> Result<(), StagingError> {
        self.restoration_attempted = true;
        if self.published {
            match fs::symlink_metadata(&self.path) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    return Err(conflict(&self.path))
                }
                Err(error) => {
                    return Err(io_error(&self.path, "inspect restore destination", error))
                }
                Ok(_) => {}
            }
            if Some(directory_contents(&self.path)?) != self.published_contents {
                return Err(conflict(&self.path));
            }
        }
        restore_staged(&self.path, &mut self.published, &mut self.backup, true)?;
        self.published = false;
        Ok(())
    }

    fn finish(&mut self) {
        self.finished = true;
        finish_staged(self.backup.take(), true);
    }
}

impl Drop for StagedDirectory {
    fn drop(&mut self) {
        if !self.finished && !self.restoration_attempted {
            let _ = self.restore();
        }
        if self.staging.is_dir() {
            let _ = fs::remove_dir_all(&self.staging);
        }
    }
}

#[derive(Debug)]
pub struct StagedRemoval {
    path: PathBuf,
    directory: bool,
    backup: Option<PathBuf>,
    finished: bool,
    restoration_attempted: bool,
}

impl StagedRemoval {
    #[must_use]
    pub fn create(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
            directory: path.is_dir(),
            backup: None,
            finished: false,
            restoration_attempted: false,
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
        Ok(())
    }

    fn restore(&mut self) -> Result<(), StagingError> {
        self.restoration_attempted = true;
        // 删除后的路径必须仍为空；外部重新创建的文件或目录不属于本事务。
        let mut published = false;
        restore_staged(&self.path, &mut published, &mut self.backup, self.directory)?;
        Ok(())
    }

    fn finish(&mut self) {
        self.finished = true;
        finish_staged(self.backup.take(), self.directory);
    }
}

impl Drop for StagedRemoval {
    fn drop(&mut self) {
        if !self.finished && !self.restoration_attempted {
            let _ = self.restore();
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
        let _ = restore_staged(path, published, backup, directory);
        *published = false;
        return Err(io_error(path, "publish", error));
    }
    *published = true;
    Ok(())
}

fn restore_staged(
    path: &Path,
    published: &mut bool,
    backup: &mut Option<PathBuf>,
    directory: bool,
) -> Result<(), StagingError> {
    if *published {
        if directory {
            fs::remove_dir_all(path)
                .map_err(|e| io_error(path, "remove published directory", e))?;
        } else {
            fs::remove_file(path).map_err(|e| io_error(path, "remove published file", e))?;
        }
        *published = false;
    }
    if let Some(backup_path) = backup.as_ref() {
        match fs::symlink_metadata(path) {
            Ok(_) => return Err(conflict(path)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(io_error(path, "inspect restore destination", e)),
        }
        fs::rename(backup_path, path).map_err(|e| io_error(path, "restore backup", e))?;
        *backup = None;
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DirectoryEntryContents {
    Directory,
    File(Vec<u8>),
    Symlink(PathBuf),
}
type DirectoryContents = BTreeMap<PathBuf, DirectoryEntryContents>;

/// A directory content snapshot captured without creating staging files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectorySnapshot {
    contents: Option<DirectoryContents>,
}
impl DirectorySnapshot {
    pub fn capture(path: &Path) -> Result<Self, StagingError> {
        let contents = match fs::symlink_metadata(path) {
            Ok(_) => Some(directory_contents(path)?),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(io_error(path, "inspect output snapshot", error)),
        };
        Ok(Self { contents })
    }
    pub fn verify(&self, path: &Path) -> Result<(), StagingError> {
        // 比较文件、目录与链接的内容身份，新增、删除和外部改写均使发布前置条件失效。
        if *self != Self::capture(path)? {
            return Err(conflict(path));
        }
        Ok(())
    }
}

fn directory_contents(root: &Path) -> Result<DirectoryContents, StagingError> {
    fn collect(
        root: &Path,
        relative: &Path,
        result: &mut DirectoryContents,
    ) -> Result<(), StagingError> {
        let path = root.join(relative);
        let metadata = fs::symlink_metadata(&path)
            .map_err(|e| io_error(&path, "inspect published output", e))?;
        if metadata.file_type().is_symlink() {
            result.insert(
                relative.to_path_buf(),
                DirectoryEntryContents::Symlink(
                    fs::read_link(&path).map_err(|e| io_error(&path, "read output link", e))?,
                ),
            );
        } else if metadata.is_dir() {
            result.insert(relative.to_path_buf(), DirectoryEntryContents::Directory);
            for entry in
                fs::read_dir(&path).map_err(|e| io_error(&path, "read published directory", e))?
            {
                let entry =
                    entry.map_err(|e| io_error(&path, "read published directory entry", e))?;
                collect(root, &relative.join(entry.file_name()), result)?;
            }
        } else {
            result.insert(
                relative.to_path_buf(),
                DirectoryEntryContents::File(
                    fs::read(&path).map_err(|e| io_error(&path, "read published file", e))?,
                ),
            );
        }
        Ok(())
    }
    let mut result = BTreeMap::new();
    collect(root, Path::new(""), &mut result)?;
    Ok(result)
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

        let mut staged =
            StagedFile::create(&path, Some(b"old".to_vec()), b"new").expect("stage file");
        staged.publish().expect("publish file");
        drop(staged);

        assert_eq!(fs::read(&path).expect("read restored"), b"old");
    }

    #[test]
    fn finished_staged_file_publishes_new_contents() {
        let root = tempfile::tempdir().expect("temp dir");
        let path = root.path().join("file.txt");
        fs::write(&path, b"old").expect("write old");

        let mut staged =
            StagedFile::create(&path, Some(b"old".to_vec()), b"new").expect("stage file");
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

        let staged =
            StagedFile::create(&path, Some(b"expected".to_vec()), b"new").expect("stage file");
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
        fs::write(staged.staging().join("new.txt"), b"new").expect("write staged file");
        staged.publish().expect("publish directory");
        drop(staged);

        assert!(path.join("old.txt").is_file());
        assert!(!path.join("new.txt").exists());
    }

    #[test]
    fn finished_directory_removal_deletes_backup_recursively() {
        let root = tempfile::tempdir().expect("temp dir");
        let path = root.path().join("removed");
        fs::create_dir(&path).expect("create directory");
        fs::write(path.join("child.txt"), b"value").expect("write child");
        let mut removal = StagedRemoval::create(&path);
        removal.publish().expect("publish removal");
        removal.finish();
        drop(removal);
        assert!(!path.exists());
        assert_eq!(fs::read_dir(root.path()).expect("read root").count(), 0);
    }

    #[test]
    fn restoring_a_file_preserves_external_contents_and_its_backup() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("file.txt");
        fs::write(&path, b"old").unwrap();
        let mut staged = StagedFile::create(&path, Some(b"old".to_vec()), b"new").unwrap();
        staged.publish().unwrap();
        fs::write(&path, b"external").unwrap();
        assert!(staged.restore().unwrap_err().is_conflict());
        let backup = staged.backup.clone().unwrap();
        drop(staged);
        assert_eq!(fs::read(&path).unwrap(), b"external");
        assert_eq!(fs::read(backup).unwrap(), b"old");
    }

    #[test]
    fn restoring_a_removed_file_preserves_external_recreation() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("file.txt");
        fs::write(&path, b"old").unwrap();
        let mut staged = StagedRemoval::create(&path);
        staged.publish().unwrap();
        fs::write(&path, b"external").unwrap();
        assert!(staged.restore().unwrap_err().is_conflict());
        let backup = staged.backup.clone().unwrap();
        drop(staged);
        assert_eq!(fs::read(&path).unwrap(), b"external");
        assert_eq!(fs::read(backup).unwrap(), b"old");
    }

    #[test]
    fn dropping_a_reported_restoration_failure_does_not_retry_publication() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("file.txt");
        fs::write(&path, b"old").unwrap();
        let mut staged = StagedFile::create(&path, Some(b"old".to_vec()), b"new").unwrap();
        staged.publish().unwrap();
        fs::write(&path, b"external").unwrap();
        assert!(staged.restore().unwrap_err().is_conflict());
        let backup = staged.backup.clone().unwrap();
        // 已经返回恢复失败后，Drop 不得再次改变对外报告的文件状态。
        fs::write(&path, b"new").unwrap();
        drop(staged);
        assert_eq!(fs::read(&path).unwrap(), b"new");
        assert_eq!(fs::read(backup).unwrap(), b"old");
    }

    #[test]
    fn restoring_output_directories_preserves_external_additions() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("output");
        fs::create_dir(&path).unwrap();
        fs::write(path.join("old.cs"), b"old").unwrap();
        let mut staged = StagedDirectory::create(&path).unwrap();
        fs::write(staged.staging().join("new.cs"), b"new").unwrap();
        staged.publish().unwrap();
        fs::write(path.join("external.cs"), b"external").unwrap();
        assert!(staged.restore().unwrap_err().is_conflict());
        let backup = staged.backup.clone().unwrap();
        drop(staged);
        assert_eq!(fs::read(path.join("external.cs")).unwrap(), b"external");
        assert_eq!(fs::read(backup.join("old.cs")).unwrap(), b"old");
    }
}
