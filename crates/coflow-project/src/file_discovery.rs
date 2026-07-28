use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct DirectoryDiscoveryError {
    kind: DirectoryDiscoveryErrorKind,
    message: String,
}

impl DirectoryDiscoveryError {
    fn new(kind: DirectoryDiscoveryErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        self.kind.path()
    }

    pub(super) fn kind(&self) -> &DirectoryDiscoveryErrorKind {
        &self.kind
    }
}

#[derive(Debug)]
pub(super) enum DirectoryDiscoveryErrorKind {
    NotDirectory {
        path: PathBuf,
    },
    Resolve {
        path: PathBuf,
        message: String,
    },
    Read {
        path: PathBuf,
        operation: &'static str,
        message: String,
    },
    OutsideRoot {
        path: PathBuf,
        canonical_root: PathBuf,
        canonical_path: PathBuf,
    },
}

impl DirectoryDiscoveryErrorKind {
    fn path(&self) -> &Path {
        match self {
            Self::NotDirectory { path }
            | Self::Resolve { path, .. }
            | Self::Read { path, .. }
            | Self::OutsideRoot { path, .. } => path,
        }
    }
}

impl fmt::Display for DirectoryDiscoveryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for DirectoryDiscoveryError {}

struct DirectoryDiscovery<'a> {
    canonical_root: PathBuf,
    visited_directories: BTreeSet<PathBuf>,
    visited_files: BTreeSet<PathBuf>,
    files: Vec<DiscoveredFile>,
    include_file: &'a dyn Fn(&Path) -> bool,
}

#[derive(Debug)]
pub(super) struct DiscoveredFile {
    pub(super) path: PathBuf,
    pub(super) canonical_path: PathBuf,
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
    discover_directory_files_with(root, &|_| true).map(|files| {
        files
            .into_iter()
            .map(|file| file.path)
            .collect::<Vec<_>>()
    })
}

pub(super) fn discover_directory_files_with(
    root: &Path,
    include_file: &dyn Fn(&Path) -> bool,
) -> Result<Vec<DiscoveredFile>, DirectoryDiscoveryError> {
    let canonical_root = canonicalize(root)?;
    if !canonical_root.is_dir() {
        return Err(DirectoryDiscoveryError::new(
            DirectoryDiscoveryErrorKind::NotDirectory {
                path: root.to_path_buf(),
            },
            format!("source directory `{}` is not a directory", root.display()),
        ));
    }
    let mut discovery = DirectoryDiscovery {
        canonical_root: canonical_root.clone(),
        visited_directories: BTreeSet::new(),
        visited_files: BTreeSet::new(),
        files: Vec::new(),
        include_file,
    };
    discovery.collect_directory(root, canonical_root)?;
    discovery
        .files
        .sort_by(|left, right| left.path.cmp(&right.path));
    Ok(discovery.files)
}

impl DirectoryDiscovery<'_> {
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
            if path.is_dir() {
                let canonical_path = canonicalize(&path)?;
                self.collect_directory(&path, canonical_path)?;
            } else if (self.include_file)(&path) {
                let canonical_path = canonicalize(&path)?;
                self.ensure_within_root(&path, &canonical_path)?;
                let metadata = fs::metadata(&canonical_path)
                    .map_err(|err| read_error(&path, "inspect", &err))?;
                if metadata.is_file() && self.visited_files.insert(canonical_path.clone()) {
                    self.files.push(DiscoveredFile {
                        path,
                        canonical_path,
                    });
                }
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
            DirectoryDiscoveryErrorKind::OutsideRoot {
                path: path.to_path_buf(),
                canonical_root: self.canonical_root.clone(),
                canonical_path: canonical_path.to_path_buf(),
            },
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
            DirectoryDiscoveryErrorKind::Resolve {
                path: path.to_path_buf(),
                message: err.to_string(),
            },
            format!("failed to resolve source path `{}`: {err}", path.display()),
        )
    })
}

fn read_error(
    path: &Path,
    operation: &'static str,
    err: &std::io::Error,
) -> DirectoryDiscoveryError {
    DirectoryDiscoveryError::new(
        DirectoryDiscoveryErrorKind::Read {
            path: path.to_path_buf(),
            operation,
            message: err.to_string(),
        },
        format!(
            "failed to {operation} source path `{}`: {err}",
            path.display()
        ),
    )
}
