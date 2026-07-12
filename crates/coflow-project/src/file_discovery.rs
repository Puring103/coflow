use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct DirectoryDiscoveryError {
    path: PathBuf,
    message: String,
}

impl DirectoryDiscoveryError {
    fn new(path: impl Into<PathBuf>, message: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            message: message.into(),
        }
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl fmt::Display for DirectoryDiscoveryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for DirectoryDiscoveryError {}

#[derive(Debug)]
struct DirectoryDiscovery {
    canonical_root: PathBuf,
    visited_directories: BTreeSet<PathBuf>,
    visited_files: BTreeSet<PathBuf>,
    files: Vec<PathBuf>,
}

/// Discovers files below a declared directory without escaping through links.
///
/// Directory symlinks and junctions that resolve within the declared root are
/// followed once. Targets outside the root are rejected, and canonical file
/// identities are returned only once even when multiple aliases exist.
///
/// # Errors
///
/// Returns an error when a path cannot be resolved or read, or when a link
/// resolves outside the declared directory root.
pub fn discover_directory_files(root: &Path) -> Result<Vec<PathBuf>, DirectoryDiscoveryError> {
    let canonical_root = canonicalize(root)?;
    if !canonical_root.is_dir() {
        return Err(DirectoryDiscoveryError::new(
            root,
            format!("source directory `{}` is not a directory", root.display()),
        ));
    }
    let mut discovery = DirectoryDiscovery {
        canonical_root: canonical_root.clone(),
        visited_directories: BTreeSet::new(),
        visited_files: BTreeSet::new(),
        files: Vec::new(),
    };
    discovery.collect_directory(root, canonical_root)?;
    discovery.files.sort();
    Ok(discovery.files)
}

impl DirectoryDiscovery {
    fn collect_directory(
        &mut self,
        dir: &Path,
        canonical_dir: PathBuf,
    ) -> Result<(), DirectoryDiscoveryError> {
        self.ensure_within_root(dir, &canonical_dir)?;
        if !self.visited_directories.insert(canonical_dir) {
            return Ok(());
        }

        let mut entries = fs::read_dir(dir)
            .map_err(|err| read_error(dir, "read", &err))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| read_error(dir, "enumerate", &err))?;
        entries.sort_by_key(fs::DirEntry::path);
        for entry in entries {
            let path = entry.path();
            let canonical_path = canonicalize(&path)?;
            self.ensure_within_root(&path, &canonical_path)?;
            let metadata =
                fs::metadata(&canonical_path).map_err(|err| read_error(&path, "inspect", &err))?;
            if metadata.is_dir() {
                self.collect_directory(&path, canonical_path)?;
            } else if metadata.is_file() && self.visited_files.insert(canonical_path) {
                self.files.push(path);
            }
        }
        Ok(())
    }

    fn ensure_within_root(
        &self,
        path: &Path,
        canonical_path: &Path,
    ) -> Result<(), DirectoryDiscoveryError> {
        if canonical_path.starts_with(&self.canonical_root) {
            return Ok(());
        }
        Err(DirectoryDiscoveryError::new(
            path,
            format!(
                "source path `{}` resolves outside declared root `{}` to `{}`",
                path.display(),
                self.canonical_root.display(),
                canonical_path.display()
            ),
        ))
    }
}

fn canonicalize(path: &Path) -> Result<PathBuf, DirectoryDiscoveryError> {
    fs::canonicalize(path).map_err(|err| {
        DirectoryDiscoveryError::new(
            path,
            format!("failed to resolve source path `{}`: {err}", path.display()),
        )
    })
}

fn read_error(path: &Path, operation: &str, err: &std::io::Error) -> DirectoryDiscoveryError {
    DirectoryDiscoveryError::new(
        path,
        format!(
            "failed to {operation} source path `{}`: {err}",
            path.display()
        ),
    )
}
