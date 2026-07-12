use std::fmt;
use std::path::{Path, PathBuf};

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
    /// Returns an error when a path is not a portable relative file path,
    /// when two paths collide under Windows filesystem semantics, or when one
    /// file path is an ancestor of another.
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
            key.push(portable_component_key(path, component)?);
        }
        paths.push((path, key));
    }

    paths.sort_by(|(_, left), (_, right)| left.cmp(right));
    for index in 1..paths.len() {
        let (left_path, left_key) = &paths[index - 1];
        let (right_path, right_key) = &paths[index];
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
    }
    Ok(())
}

fn portable_component_key(path: &Path, component: &str) -> Result<String, ArtifactSetError> {
    if matches!(component.chars().last(), Some(' ' | '.')) {
        return Err(ArtifactSetError::new(format!(
            "artifact path `{}` contains a component ending in a space or period, which is not portable to Windows",
            path.display()
        )));
    }
    if component.chars().any(is_windows_forbidden_character) {
        return Err(ArtifactSetError::new(format!(
            "artifact path `{}` contains a component with a character forbidden by Windows",
            path.display()
        )));
    }
    if is_windows_reserved_device_name(component) {
        return Err(ArtifactSetError::new(format!(
            "artifact path `{}` contains a Windows reserved device name",
            path.display()
        )));
    }
    Ok(component.to_lowercase())
}

fn is_windows_forbidden_character(character: char) -> bool {
    character.is_ascii_control()
        || matches!(
            character,
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
        )
}

fn is_windows_reserved_device_name(component: &str) -> bool {
    let device = component.split('.').next().unwrap_or_default();
    if ["CON", "PRN", "AUX", "NUL"]
        .iter()
        .any(|name| device.eq_ignore_ascii_case(name))
    {
        return true;
    }

    let mut characters = device.chars();
    let prefix = [characters.next(), characters.next(), characters.next()];
    let suffix = characters.next();
    if characters.next().is_some() {
        return false;
    }
    let is_com = matches!(prefix, [Some('C' | 'c'), Some('O' | 'o'), Some('M' | 'm')]);
    let is_lpt = matches!(prefix, [Some('L' | 'l'), Some('P' | 'p'), Some('T' | 't')]);
    (is_com || is_lpt)
        && matches!(
            suffix,
            Some('1'..='9' | '\u{00B9}' | '\u{00B2}' | '\u{00B3}')
        )
}

fn is_component_prefix(left: &[String], right: &[String]) -> bool {
    left.len() < right.len() && right.starts_with(left)
}

fn path_prefix_error(parent: &Path, child: &Path) -> ArtifactSetError {
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
    #![allow(clippy::expect_used)]

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
        for right in ["data/item.json", "Data/Item.json", "data/item.JSON"] {
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
    fn rejects_windows_incompatible_path_components() {
        for path in [
            "CON",
            "data/Aux.json",
            "data/COM1.txt",
            "data/LPT\u{00B2}.txt",
            "data/name?.json",
            "data/name\u{0001}.json",
            "data/name.",
            "data/name ",
        ] {
            let error = ArtifactSet::new(vec![ArtifactFile::text(path, "")])
                .expect_err("Windows-incompatible artifact path");
            assert!(
                error.to_string().contains("Windows"),
                "unexpected error for `{path}`: {error}"
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
