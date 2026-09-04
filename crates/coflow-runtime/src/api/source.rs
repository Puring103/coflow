use crate::{data_model::LoadedRecordDraft, DiagnosticSet};
use coflow_language::cft::CftSchema;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CfdSourcePath(PathBuf);

impl CfdSourcePath {
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self(path.into())
    }

    #[must_use]
    pub const fn path(&self) -> &PathBuf {
        &self.0
    }
}

#[derive(Debug, Clone)]
pub struct CfdSource {
    pub location: CfdSourcePath,
    pub display_name: String,
}

#[derive(Debug, Clone, Copy)]
pub struct CfdLoadContext<'a> {
    pub schema: &'a CftSchema,
    /// Host-provided source text that should be loaded instead of the backing file.
    /// Text compensations may use this for unsaved documents and dry-run validation.
    pub source_text: Option<&'a str>,
}

#[derive(Debug, Clone)]
pub struct LoadedCfdSource {
    pub records: Vec<LoadedRecordDraft>,
    pub diagnostics: DiagnosticSet,
}
