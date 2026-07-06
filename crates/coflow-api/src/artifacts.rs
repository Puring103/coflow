use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactContentKind {
    Text,
    Bytes,
    Json,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactSet {
    pub files: Vec<ArtifactFile>,
    pub metadata: BTreeMap<String, String>,
}

impl ArtifactSet {
    #[must_use]
    pub fn new(files: Vec<ArtifactFile>) -> Self {
        Self {
            files,
            metadata: BTreeMap::new(),
        }
    }
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

    #[must_use]
    pub fn json(relative_path: impl Into<PathBuf>, value: serde_json::Value) -> Self {
        Self {
            relative_path: relative_path.into(),
            content: ArtifactContent::Json(value),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArtifactContent {
    Text(String),
    Bytes(Vec<u8>),
    Json(serde_json::Value),
}
