//! Small, data-only SPI for target-language code generators.
//!
//! A generator receives one immutable schema/model snapshot and returns only
//! target-language source artifacts; publication and filesystem access stay in
//! the application layer.

use crate::CfdDataModel;
use coflow_language::CftSchema;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceManifestEntry {
    pub logical_path: String,
    pub origin: SourceOrigin,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceOrigin {
    Project,
    Dimension {
        dimension: String,
        source_type: String,
        field: String,
    },
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
        let mut portable_paths = Vec::with_capacity(files.len());
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
            let mut key = Vec::new();
            for component in file.relative_path.components() {
                let std::path::Component::Normal(component) = component else {
                    return Err(CodegenError::InvalidArtifactPath(
                        file.relative_path.clone(),
                    ));
                };
                let Some(component) = component.to_str() else {
                    return Err(CodegenError::InvalidArtifactPath(
                        file.relative_path.clone(),
                    ));
                };
                if !portable_artifact_component(component) {
                    return Err(CodegenError::InvalidArtifactPath(
                        file.relative_path.clone(),
                    ));
                }
                key.push(component.to_lowercase());
            }
            portable_paths.push((&file.relative_path, key));
        }
        portable_paths.sort_by(|left, right| left.1.cmp(&right.1));
        for pair in portable_paths.windows(2) {
            let [(_, left), (right_path, right)] = pair else {
                continue;
            };
            if left == right {
                return Err(CodegenError::DuplicateArtifactPath((*right_path).clone()));
            }
            if left.len() < right.len() && right.starts_with(left) {
                return Err(CodegenError::InvalidArtifactPath((*right_path).clone()));
            }
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

fn portable_artifact_component(component: &str) -> bool {
    if matches!(component.chars().last(), Some(' ' | '.'))
        || component.chars().any(|character| {
            character.is_ascii_control()
                || matches!(
                    character,
                    '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
                )
        })
    {
        return false;
    }
    let device = component.split('.').next().unwrap_or_default();
    if ["CON", "PRN", "AUX", "NUL"]
        .iter()
        .any(|name| device.eq_ignore_ascii_case(name))
    {
        return false;
    }
    let mut characters = device.chars();
    let prefix = [characters.next(), characters.next(), characters.next()];
    let suffix = characters.next();
    if characters.next().is_some() {
        return true;
    }
    let is_com = matches!(prefix, [Some('C' | 'c'), Some('O' | 'o'), Some('M' | 'm')]);
    let is_lpt = matches!(prefix, [Some('L' | 'l'), Some('P' | 'p'), Some('T' | 't')]);
    !(is_com || is_lpt)
        || !matches!(
            suffix,
            Some('1'..='9' | '\u{00B9}' | '\u{00B2}' | '\u{00B3}')
        )
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
