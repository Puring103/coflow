//! Small, data-only SPI for target-language code generators.
//!
//! Source loading and data export are intentionally absent.  A generator gets
//! one immutable snapshot and returns only source artifacts; publication and
//! filesystem access stay in the application layer.

use coflow_language::CftSchema;
use coflow_runtime::CfdDataModel;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceManifestEntry {
    pub logical_path: String,
    pub origin: SourceOrigin,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceOrigin {
    Project,
    Dimension { dimension: String, field: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodegenTarget {
    pub id: String,
    pub output_dir: PathBuf,
    pub options: Value,
}

impl CodegenTarget {
    #[must_use]
    pub fn new(id: impl Into<String>, output_dir: impl Into<PathBuf>, options: Value) -> Self {
        Self {
            id: id.into(),
            output_dir: output_dir.into(),
            options,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct CodegenInput<'a> {
    pub schema: &'a CftSchema,
    pub model: Option<&'a CfdDataModel>,
    pub sources: &'a [SourceManifestEntry],
    pub target: &'a CodegenTarget,
    pub id_as_enum_lock: &'a Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodegenDescriptor {
    pub id: &'static str,
    pub language: &'static str,
    pub file_extensions: &'static [&'static str],
    pub runtime_package: &'static str,
    pub runtime_version: &'static str,
    pub needs_model: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeArtifactFile {
    pub relative_path: PathBuf,
    pub contents: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeArtifactSet {
    files: Vec<CodeArtifactFile>,
}

impl CodeArtifactSet {
    /// Creates an artifact set and rejects traversal, absolute paths and
    /// duplicate files before the application can stage it.
    pub fn new(mut files: Vec<CodeArtifactFile>) -> Result<Self, CodegenError> {
        files.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
        let mut previous: Option<&Path> = None;
        for file in &files {
            if file.relative_path.as_os_str().is_empty()
                || file.relative_path.is_absolute()
                || file
                    .relative_path
                    .components()
                    .any(|component| !matches!(component, std::path::Component::Normal(_)))
            {
                return Err(CodegenError::InvalidArtifactPath(
                    file.relative_path.clone(),
                ));
            }
            if previous == Some(file.relative_path.as_path()) {
                return Err(CodegenError::DuplicateArtifactPath(
                    file.relative_path.clone(),
                ));
            }
            previous = Some(&file.relative_path);
        }
        Ok(Self { files })
    }

    #[must_use]
    pub fn files(&self) -> &[CodeArtifactFile] {
        &self.files
    }

    #[must_use]
    pub fn into_files(self) -> Vec<CodeArtifactFile> {
        self.files
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodegenError {
    InvalidArtifactPath(PathBuf),
    DuplicateArtifactPath(PathBuf),
    Message(String),
}

impl fmt::Display for CodegenError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidArtifactPath(path) => {
                write!(formatter, "invalid code artifact path `{}`", path.display())
            }
            Self::DuplicateArtifactPath(path) => {
                write!(
                    formatter,
                    "duplicate code artifact path `{}`",
                    path.display()
                )
            }
            Self::Message(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for CodegenError {}

pub trait CodeGenerator: Send + Sync + fmt::Debug {
    fn descriptor(&self) -> &'static CodegenDescriptor;
    fn generate(&self, input: CodegenInput<'_>) -> Result<CodeArtifactSet, CodegenError>;
}

#[derive(Debug, Default)]
pub struct CodegenRegistry {
    generators: BTreeMap<String, Arc<dyn CodeGenerator>>,
}

impl CodegenRegistry {
    pub fn register<G>(&mut self, generator: G) -> Result<(), CodegenError>
    where
        G: CodeGenerator + 'static,
    {
        let id = generator.descriptor().id.to_string();
        if self.generators.contains_key(&id) {
            return Err(CodegenError::Message(format!(
                "code generator `{id}` is already registered"
            )));
        }
        self.generators.insert(id, Arc::new(generator));
        Ok(())
    }

    #[must_use]
    pub fn get(&self, id: &str) -> Option<&dyn CodeGenerator> {
        self.generators.get(id).map(Arc::as_ref)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &dyn CodeGenerator)> {
        self.generators
            .iter()
            .map(|(id, generator)| (id.as_str(), generator.as_ref()))
    }
}

/// Shared source identity used by build, editor overlays and generated code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectSnapshot {
    pub revision: u64,
    pub sources: Arc<[SourceManifestEntry]>,
}
