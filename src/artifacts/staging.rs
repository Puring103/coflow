use super::diagnostic_set;
use super::fault::{self, Point};
use coflow_api::{ArtifactContent, ArtifactSet, DiagnosticSet};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug)]
pub struct StagedArtifactDir {
    requested_dir: PathBuf,
    staging_dir: PathBuf,
    requested_staging: Option<RequestedArtifactDir>,
    slot: String,
    sealed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublishedArtifactDir {
    pub requested_dir: PathBuf,
    pub generation_dir: PathBuf,
}

#[derive(Debug)]
pub(super) struct RequestedArtifactDir {
    requested_dir: PathBuf,
    staging_dir: PathBuf,
    backup_dir: Option<PathBuf>,
    published: bool,
    active: bool,
}

pub(super) fn stage_artifact_set(
    state_dir: &Path,
    slot: &str,
    dir: &Path,
    artifacts: ArtifactSet,
) -> Result<StagedArtifactDir, DiagnosticSet> {
    let staged = StagedArtifactDir::create(state_dir, slot, dir)?;
    let requested_staging_path = staged.requested_path()?.to_path_buf();
    for artifact in artifacts.into_files() {
        let path = staged.path().join(&artifact.relative_path);
        let requested_path = requested_staging_path.join(&artifact.relative_path);
        if let Some(parent) = path.parent() {
            fault::check(Point::CreateArtifactParent).map_err(|err| {
                diagnostic_set(
                    dir,
                    format!("failed to create `{}`: {err}", parent.display()),
                )
            })?;
            fs::create_dir_all(parent).map_err(|err| {
                diagnostic_set(
                    dir,
                    format!("failed to create `{}`: {err}", parent.display()),
                )
            })?;
        }
        if let Some(parent) = requested_path.parent() {
            fault::check(Point::CreateArtifactParent).map_err(|err| {
                diagnostic_set(
                    dir,
                    format!("failed to create `{}`: {err}", parent.display()),
                )
            })?;
            fs::create_dir_all(parent).map_err(|err| {
                diagnostic_set(
                    dir,
                    format!("failed to create `{}`: {err}", parent.display()),
                )
            })?;
        }
        let contents = match artifact.content {
            ArtifactContent::Text(contents) => contents.into_bytes(),
            ArtifactContent::Bytes(bytes) => bytes,
        };
        write_verified_file(&path, &contents)?;
        write_verified_file(&requested_path, &contents)?;
    }
    preserve_unity_meta_files(dir, &requested_staging_path)?;
    fault::check(Point::SyncStagingTree)
        .and_then(|()| sync_directory_tree(staged.path()))
        .map_err(|err| diagnostic_set(dir, format!("failed to sync staged artifacts: {err}")))?;
    fault::check(Point::SyncStagingTree)
        .and_then(|()| sync_directory_tree(&requested_staging_path))
        .map_err(|err| {
            diagnostic_set(
                dir,
                format!("failed to sync requested output staging: {err}"),
            )
        })?;
    Ok(staged)
}

pub(super) fn artifact_set_matches_requested_dir(
    artifacts: &ArtifactSet,
    requested_dir: &Path,
) -> Result<bool, DiagnosticSet> {
    if !requested_dir.is_dir() {
        return Ok(false);
    }
    let mut expected = artifacts
        .files()
        .iter()
        .map(|artifact| {
            let contents = match &artifact.content {
                ArtifactContent::Text(contents) => contents.as_bytes().to_vec(),
                ArtifactContent::Bytes(bytes) => bytes.clone(),
            };
            (artifact.relative_path.clone(), contents)
        })
        .collect::<BTreeMap<_, _>>();
    Ok(compare_requested_tree(requested_dir, requested_dir, &mut expected)? && expected.is_empty())
}

fn compare_requested_tree(
    root: &Path,
    directory: &Path,
    expected: &mut BTreeMap<PathBuf, Vec<u8>>,
) -> Result<bool, DiagnosticSet> {
    let entries = fs::read_dir(directory).map_err(|err| {
        diagnostic_set(
            directory,
            format!(
                "failed to inspect output directory `{}`: {err}",
                directory.display()
            ),
        )
    })?;
    for entry in entries {
        let entry = entry.map_err(|err| {
            diagnostic_set(
                directory,
                format!(
                    "failed to inspect output directory `{}`: {err}",
                    directory.display()
                ),
            )
        })?;
        let path = entry.path();
        let file_type = entry.file_type().map_err(|err| {
            diagnostic_set(
                &path,
                format!("failed to inspect `{}`: {err}", path.display()),
            )
        })?;
        if file_type.is_dir() {
            if !compare_requested_tree(root, &path, expected)? {
                return Ok(false);
            }
            continue;
        }
        if !file_type.is_file() {
            return Ok(false);
        }
        let relative = path.strip_prefix(root).map_err(|err| {
            diagnostic_set(
                &path,
                format!("failed to resolve `{}`: {err}", path.display()),
            )
        })?;
        if is_unity_meta_path(&path) && !expected.contains_key(relative) {
            continue;
        }
        let Some(contents) = expected.remove(relative) else {
            return Ok(false);
        };
        let actual = fs::read(&path).map_err(|err| {
            diagnostic_set(&path, format!("failed to read `{}`: {err}", path.display()))
        })?;
        if actual != contents {
            return Ok(false);
        }
    }
    Ok(true)
}

fn preserve_unity_meta_files(
    requested_dir: &Path,
    requested_staging_dir: &Path,
) -> Result<(), DiagnosticSet> {
    if !requested_dir.is_dir() {
        return Ok(());
    }
    preserve_unity_meta_tree(requested_dir, requested_staging_dir)
}

fn preserve_unity_meta_tree(source_dir: &Path, staging_dir: &Path) -> Result<(), DiagnosticSet> {
    let mut entries = fs::read_dir(source_dir)
        .map_err(|err| {
            diagnostic_set(
                source_dir,
                format!(
                    "failed to inspect output directory `{}` for Unity metadata: {err}",
                    source_dir.display()
                ),
            )
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| {
            diagnostic_set(
                source_dir,
                format!(
                    "failed to inspect output directory `{}` for Unity metadata: {err}",
                    source_dir.display()
                ),
            )
        })?;
    entries.sort_by_key(fs::DirEntry::file_name);

    for entry in entries {
        let source = entry.path();
        let file_type = entry.file_type().map_err(|err| {
            diagnostic_set(
                &source,
                format!(
                    "failed to inspect output entry `{}`: {err}",
                    source.display()
                ),
            )
        })?;
        if file_type.is_dir() {
            let nested_staging_dir = staging_dir.join(entry.file_name());
            if nested_staging_dir.exists() && !nested_staging_dir.is_dir() {
                continue;
            }
            preserve_unity_meta_tree(&source, &nested_staging_dir)?;
            continue;
        }
        if !file_type.is_file() || !is_unity_meta_path(&source) {
            continue;
        }
        let destination = staging_dir.join(entry.file_name());
        if destination.exists() {
            continue;
        }
        if let Some(parent) = destination.parent() {
            fault::check(Point::CreateArtifactParent).map_err(|err| {
                diagnostic_set(
                    parent,
                    format!("failed to create `{}`: {err}", parent.display()),
                )
            })?;
            fs::create_dir_all(parent).map_err(|err| {
                diagnostic_set(
                    parent,
                    format!("failed to create `{}`: {err}", parent.display()),
                )
            })?;
        }
        let contents = fs::read(&source).map_err(|err| {
            diagnostic_set(
                &source,
                format!(
                    "failed to read Unity metadata file `{}`: {err}",
                    source.display()
                ),
            )
        })?;
        write_verified_file(&destination, &contents)?;
    }
    Ok(())
}

fn is_unity_meta_path(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("meta"))
}

fn write_verified_file(path: &Path, contents: &[u8]) -> Result<(), DiagnosticSet> {
    fault::check(Point::CreateArtifactFile).map_err(|err| {
        diagnostic_set(
            path,
            format!("failed to create `{}`: {err}", path.display()),
        )
    })?;
    let mut file = fs::File::create(path).map_err(|err| {
        diagnostic_set(
            path,
            format!("failed to create `{}`: {err}", path.display()),
        )
    })?;
    fault::check(Point::WriteArtifactFile)
        .and_then(|()| file.write_all(contents))
        .map_err(|err| {
            diagnostic_set(path, format!("failed to write `{}`: {err}", path.display()))
        })?;
    fault::check(Point::SyncArtifactFile)
        .and_then(|()| file.sync_all())
        .map_err(|err| {
            diagnostic_set(path, format!("failed to sync `{}`: {err}", path.display()))
        })?;
    drop(file);

    let written = fault::check(Point::ReadArtifactFile)
        .and_then(|()| fs::read(path))
        .map_err(|err| {
            diagnostic_set(
                path,
                format!("failed to verify `{}`: {err}", path.display()),
            )
        })?;
    if written != contents {
        return Err(diagnostic_set(
            path,
            format!("verification failed for `{}`", path.display()),
        ));
    }
    Ok(())
}

impl StagedArtifactDir {
    pub fn create(
        state_dir: &Path,
        slot: &str,
        requested_dir: &Path,
    ) -> Result<Self, DiagnosticSet> {
        let requested_staging = RequestedArtifactDir::create(requested_dir)?;
        let parent = state_dir.join("staging");
        fault::check(Point::CreateOutputParent).map_err(|err| {
            diagnostic_set(
                requested_dir,
                format!("failed to create `{}`: {err}", parent.display()),
            )
        })?;
        fs::create_dir_all(&parent).map_err(|err| {
            diagnostic_set(
                requested_dir,
                format!("failed to create `{}`: {err}", parent.display()),
            )
        })?;
        let staging_dir = unique_artifact_path(&parent, slot);
        fault::check(Point::CreateStagingDirectory).map_err(|err| {
            diagnostic_set(
                requested_dir,
                format!("failed to create `{}`: {err}", staging_dir.display()),
            )
        })?;
        fs::create_dir(&staging_dir).map_err(|err| {
            diagnostic_set(
                requested_dir,
                format!("failed to create `{}`: {err}", staging_dir.display()),
            )
        })?;
        Ok(Self {
            requested_dir: requested_dir.to_path_buf(),
            staging_dir,
            requested_staging: Some(requested_staging),
            slot: slot.to_string(),
            sealed: false,
        })
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.staging_dir
    }

    pub(super) fn requested_dir(&self) -> &Path {
        &self.requested_dir
    }

    pub(super) fn requested_output_is_unchanged(&self) -> bool {
        self.requested_path().is_ok_and(|staging| {
            directory_trees_equal(staging, &self.requested_dir).unwrap_or(false)
        })
    }

    fn requested_path(&self) -> Result<&Path, DiagnosticSet> {
        self.requested_staging
            .as_ref()
            .map(|staging| staging.staging_dir.as_path())
            .ok_or_else(|| {
                diagnostic_set(
                    &self.requested_dir,
                    "requested output staging is unavailable before sealing",
                )
            })
    }

    pub(super) fn seal(
        mut self,
    ) -> Result<(PublishedArtifactDir, RequestedArtifactDir), DiagnosticSet> {
        let state_dir = self
            .staging_dir
            .parent()
            .and_then(Path::parent)
            .unwrap_or_else(|| Path::new("."));
        let generation_parent = state_dir.join("generations");
        fs::create_dir_all(&generation_parent).map_err(|err| {
            diagnostic_set(
                &self.requested_dir,
                format!("failed to create `{}`: {err}", generation_parent.display()),
            )
        })?;
        let generation_dir = unique_artifact_path(&generation_parent, &self.slot);
        fault::check(Point::SealGeneration)
            .and_then(|()| fs::rename(&self.staging_dir, &generation_dir))
            .map_err(|err| {
                diagnostic_set(
                    &self.requested_dir,
                    format!(
                        "failed to seal artifact generation `{}` as `{}`: {err}",
                        self.staging_dir.display(),
                        generation_dir.display()
                    ),
                )
            })?;
        self.sealed = true;
        let parent = generation_dir.parent().unwrap_or_else(|| Path::new("."));
        fault::check(Point::SyncGenerationParent)
            .and_then(|()| sync_directory(parent))
            .map_err(|err| {
                let _ = fs::remove_dir_all(&generation_dir);
                diagnostic_set(
                    &generation_dir,
                    format!(
                        "failed to sync artifact generation `{}`: {err}",
                        generation_dir.display()
                    ),
                )
            })?;
        let Some(requested_staging) = self.requested_staging.take() else {
            let _ = fs::remove_dir_all(&generation_dir);
            return Err(diagnostic_set(
                &self.requested_dir,
                "requested output staging is unavailable while sealing",
            ));
        };
        let published = PublishedArtifactDir {
            requested_dir: self.requested_dir.clone(),
            generation_dir,
        };
        Ok((published, requested_staging))
    }
}

impl Drop for StagedArtifactDir {
    fn drop(&mut self) {
        if !self.sealed {
            let _ = fs::remove_dir_all(&self.staging_dir);
        }
    }
}

impl RequestedArtifactDir {
    fn create(requested_dir: &Path) -> Result<Self, DiagnosticSet> {
        let parent = requested_dir.parent().unwrap_or_else(|| Path::new("."));
        fault::check(Point::CreateOutputParent).map_err(|err| {
            diagnostic_set(
                requested_dir,
                format!("failed to create `{}`: {err}", parent.display()),
            )
        })?;
        fs::create_dir_all(parent).map_err(|err| {
            diagnostic_set(
                requested_dir,
                format!("failed to create `{}`: {err}", parent.display()),
            )
        })?;
        let staging_dir = unique_requested_path(requested_dir, "staging");
        fault::check(Point::CreateStagingDirectory).map_err(|err| {
            diagnostic_set(
                requested_dir,
                format!("failed to create `{}`: {err}", staging_dir.display()),
            )
        })?;
        fs::create_dir(&staging_dir).map_err(|err| {
            diagnostic_set(
                requested_dir,
                format!("failed to create `{}`: {err}", staging_dir.display()),
            )
        })?;
        Ok(Self {
            requested_dir: requested_dir.to_path_buf(),
            staging_dir,
            backup_dir: None,
            published: false,
            active: false,
        })
    }

    pub(super) fn publish(&mut self) -> Result<(), DiagnosticSet> {
        if self.requested_dir.exists() && !self.requested_dir.is_dir() {
            return Err(diagnostic_set(
                &self.requested_dir,
                format!(
                    "failed to replace output dir `{}`: target is not a directory",
                    self.requested_dir.display()
                ),
            ));
        }

        if self.requested_dir.exists() {
            let backup_dir = unique_requested_path(&self.requested_dir, "backup");
            fault::check(Point::MoveRequestedOutputToBackup)
                .and_then(|()| fs::rename(&self.requested_dir, &backup_dir))
                .map_err(|err| {
                    diagnostic_set(
                        &self.requested_dir,
                        format!(
                            "failed to move old output dir `{}` to `{}`: {err}",
                            self.requested_dir.display(),
                            backup_dir.display()
                        ),
                    )
                })?;
            self.backup_dir = Some(backup_dir);
        }

        let publish_result = fault::check(Point::PublishRequestedOutput)
            .and_then(|()| fs::rename(&self.staging_dir, &self.requested_dir));
        if let Err(err) = publish_result {
            self.restore_backup();
            return Err(diagnostic_set(
                &self.requested_dir,
                format!(
                    "failed to publish staged output `{}` as `{}`: {err}",
                    self.staging_dir.display(),
                    self.requested_dir.display()
                ),
            ));
        }
        self.published = true;
        Ok(())
    }

    pub(super) fn activate(&mut self) {
        self.active = true;
        if let Some(backup_dir) = self.backup_dir.take() {
            let _ = fs::remove_dir_all(backup_dir);
        }
    }

    fn restore_backup(&mut self) {
        if self.requested_dir.is_dir() {
            let _ = fs::remove_dir_all(&self.requested_dir);
        }
        if let Some(backup_dir) = self.backup_dir.take() {
            let _ = fs::rename(backup_dir, &self.requested_dir);
        }
        self.published = false;
    }
}

impl Drop for RequestedArtifactDir {
    fn drop(&mut self) {
        if self.published && !self.active {
            self.restore_backup();
        } else if !self.published {
            let _ = fs::remove_dir_all(&self.staging_dir);
        }
    }
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> std::io::Result<()> {
    fs::File::open(path)?.sync_all()
}

#[cfg(unix)]
fn sync_directory_tree(path: &Path) -> std::io::Result<()> {
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            sync_directory_tree(&entry.path())?;
        }
    }
    sync_directory(path)
}

#[cfg(windows)]
#[allow(clippy::unnecessary_wraps)] // Windows has no directory fsync equivalent.
const fn sync_directory(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(windows)]
#[allow(clippy::unnecessary_wraps)] // Keeps the platform implementations interchangeable.
const fn sync_directory_tree(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

fn unique_artifact_path(parent: &Path, slot: &str) -> PathBuf {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    parent.join(format!("{slot}-{}-{suffix}", std::process::id()))
}

fn unique_requested_path(target: &Path, kind: &str) -> PathBuf {
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

fn directory_trees_equal(left: &Path, right: &Path) -> std::io::Result<bool> {
    if !left.is_dir() || !right.is_dir() {
        return Ok(false);
    }

    let mut left_entries = fs::read_dir(left)?.collect::<Result<Vec<_>, _>>()?;
    let mut right_entries = fs::read_dir(right)?.collect::<Result<Vec<_>, _>>()?;
    left_entries.sort_by_key(fs::DirEntry::file_name);
    right_entries.sort_by_key(fs::DirEntry::file_name);

    if left_entries.len() != right_entries.len() {
        return Ok(false);
    }

    for (left_entry, right_entry) in left_entries.iter().zip(&right_entries) {
        if left_entry.file_name() != right_entry.file_name() {
            return Ok(false);
        }

        let left_type = left_entry.file_type()?;
        let right_type = right_entry.file_type()?;
        if left_type.is_dir() && right_type.is_dir() {
            if !directory_trees_equal(&left_entry.path(), &right_entry.path())? {
                return Ok(false);
            }
        } else if left_type.is_file() && right_type.is_file() {
            if !files_equal(&left_entry.path(), &right_entry.path())? {
                return Ok(false);
            }
        } else {
            return Ok(false);
        }
    }

    Ok(true)
}

fn files_equal(left: &Path, right: &Path) -> std::io::Result<bool> {
    if fs::metadata(left)?.len() != fs::metadata(right)?.len() {
        return Ok(false);
    }

    let mut left = BufReader::new(fs::File::open(left)?);
    let mut right = BufReader::new(fs::File::open(right)?);
    let mut left_buffer = vec![0; 64 * 1024].into_boxed_slice();
    let mut right_buffer = vec![0; 64 * 1024].into_boxed_slice();
    loop {
        let left_read = left.read(&mut left_buffer)?;
        let right_read = right.read(&mut right_buffer)?;
        if left_read != right_read {
            return Ok(false);
        }
        if left_buffer[..left_read] != right_buffer[..left_read] {
            return Ok(false);
        }
        if left_read == 0 {
            return Ok(true);
        }
    }
}
