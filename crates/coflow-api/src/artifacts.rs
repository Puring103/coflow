use std::fmt;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactContentKind {
    Text,
    Bytes,
    Json,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactSet {
    files: Vec<ArtifactFile>,
}

impl ArtifactSet {
    /// Creates a complete set after validating its portable path namespace.
    ///
    /// # Errors
    ///
    /// Returns an error when a path is not a relative file path, when two
    /// paths collide under Windows filesystem semantics, or when one file
    /// path is an ancestor of another.
    pub fn new(files: Vec<ArtifactFile>) -> Result<Self, ArtifactSetError> {
        validate_paths(&files)?;
        Ok(Self { files })
    }

    #[must_use]
    pub fn files(&self) -> &[ArtifactFile] {
        &self.files
    }

    #[must_use]
    pub fn into_files(self) -> Vec<ArtifactFile> {
        self.files
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactSetError {
    message: String,
}

impl ArtifactSetError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for ArtifactSetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.message.fmt(f)
    }
}

impl std::error::Error for ArtifactSetError {}

fn validate_paths(files: &[ArtifactFile]) -> Result<(), ArtifactSetError> {
    let mut paths = Vec::with_capacity(files.len());
    for file in files {
        let path = &file.relative_path;
        if path.as_os_str().is_empty() || path.is_absolute() {
            return Err(ArtifactSetError::new(format!(
                "artifact path `{}` must be a non-empty relative file path",
                path.display()
            )));
        }

        let mut key = Vec::new();
        for component in path.components() {
            let std::path::Component::Normal(component) = component else {
                return Err(ArtifactSetError::new(format!(
                    "artifact path `{}` must contain only normal path components",
                    path.display()
                )));
            };
            let component = component.to_str().ok_or_else(|| {
                ArtifactSetError::new(format!(
                    "artifact path `{}` must be valid Unicode",
                    path.display()
                ))
            })?;
            key.push(component.trim_end_matches([' ', '.']).to_lowercase());
        }
        if key.iter().any(String::is_empty) {
            return Err(ArtifactSetError::new(format!(
                "artifact path `{}` contains a component that is empty under Windows filesystem semantics",
                path.display()
            )));
        }
        paths.push((path, key));
    }

    for (index, (left_path, left_key)) in paths.iter().enumerate() {
        for (right_path, right_key) in paths.iter().skip(index + 1) {
            if left_key == right_key {
                let message = if left_path == right_path {
                    format!("duplicate artifact path `{}`", left_path.display())
                } else {
                    format!(
                        "artifact paths `{}` and `{}` collide under Windows filesystem semantics",
                        left_path.display(),
                        right_path.display()
                    )
                };
                return Err(ArtifactSetError::new(message));
            }
            if is_component_prefix(left_key, right_key) {
                return Err(path_prefix_error(left_path, right_path));
            }
            if is_component_prefix(right_key, left_key) {
                return Err(path_prefix_error(right_path, left_path));
            }
        }
    }
    Ok(())
}

fn is_component_prefix(left: &[String], right: &[String]) -> bool {
    left.len() < right.len() && right.starts_with(left)
}

fn path_prefix_error(parent: &PathBuf, child: &PathBuf) -> ArtifactSetError {
    ArtifactSetError::new(format!(
        "artifact path `{}` cannot be both a file and the parent of `{}`",
        parent.display(),
        child.display()
    ))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactFile {
    pub relative_path: PathBuf,
    pub content: ArtifactContent,
}

impl ArtifactFile {
    #[must_use]
    pub fn text(relative_path: impl Into<PathBuf>, contents: impl Into<String>) -> Self {
        Self {
            relative_path: relative_path.into(),
            content: ArtifactContent::Text(contents.into()),
        }
    }

    #[must_use]
    pub fn bytes(relative_path: impl Into<PathBuf>, contents: Vec<u8>) -> Self {
        Self {
            relative_path: relative_path.into(),
            content: ArtifactContent::Bytes(contents),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArtifactContent {
    Text(String),
    Bytes(Vec<u8>),
}

#[cfg(test)]
mod tests {
    use super::{ArtifactFile, ArtifactSet};

    #[test]
    fn accepts_distinct_relative_file_paths() {
        let set = ArtifactSet::new(vec![
            ArtifactFile::text("tables/Item.json", "[]"),
            ArtifactFile::text("code/Item.cs", ""),
        ])
        .expect("valid artifact set");

        assert_eq!(set.files().len(), 2);
    }

    #[test]
    fn rejects_non_file_paths() {
        for path in ["", ".", "/escape", "../escape", "nested/../escape"] {
            let error = ArtifactSet::new(vec![ArtifactFile::text(path, "")])
                .expect_err("invalid artifact path");
            assert!(
                error.to_string().contains("artifact path"),
                "unexpected error: {error}"
            );
        }
    }

    #[test]
    fn rejects_duplicate_and_windows_equivalent_paths() {
        for right in [
            "data/item.json",
            "Data/Item.json",
            "data/item.JSON",
            "data/item.json.",
        ] {
            let error = ArtifactSet::new(vec![
                ArtifactFile::text("data/item.json", ""),
                ArtifactFile::text(right, ""),
            ])
            .expect_err("colliding artifact paths");
            assert!(
                error.to_string().contains("duplicate")
                    || error.to_string().contains("Windows filesystem semantics"),
                "unexpected error: {error}"
            );
        }
    }

    #[test]
    fn rejects_file_directory_prefix_collisions() {
        let error = ArtifactSet::new(vec![
            ArtifactFile::text("data", ""),
            ArtifactFile::text("DATA/items.json", ""),
        ])
        .expect_err("file/directory collision");

        assert!(error.to_string().contains("both a file and the parent"));
    }
}
