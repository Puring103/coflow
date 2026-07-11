mod snapshot;
mod worker;

use coflow_cfd::{parse_cfd, CfdAst};
use coflow_cft::CftContainer;
use coflow_project::{normalize_path, Project};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use crate::path_from_file_uri;
use crate::state::{LspBuild, LspDocument};

pub(crate) use snapshot::{
    build_snapshot, ValidationInput, ValidationRevision, ValidationSnapshot,
};
pub(crate) use worker::ValidationWorker;

pub(crate) struct LspValidationCore {
    project: Project,
    open_documents: BTreeMap<PathBuf, OpenDocument>,
    published_uris: BTreeSet<String>,
    revision: ValidationRevision,
    snapshot: Option<ValidationSnapshot>,
}

#[derive(Debug, Clone)]
pub(crate) struct OpenDocument {
    pub(crate) uri: String,
    pub(crate) text: String,
    pub(crate) version: Option<i64>,
}

pub(crate) struct DiagnosticPublication {
    pub(crate) uri: String,
    pub(crate) diagnostics: Vec<Value>,
    pub(crate) version: Option<i64>,
}

pub(crate) enum LspRequestDocument<'a> {
    Cfd(CfdRequestDocument<'a>),
    Cft {
        build: &'a LspBuild,
        document: &'a LspDocument,
    },
    Missing,
}

pub(crate) struct CfdRequestDocument<'a> {
    pub(crate) source: String,
    pub(crate) ast: CfdAst,
    pub(crate) schema: Option<&'a CftContainer>,
    pub(crate) build: Option<&'a LspBuild>,
}

impl LspValidationCore {
    pub(crate) const fn new(project: Project) -> Self {
        Self {
            project,
            open_documents: BTreeMap::new(),
            published_uris: BTreeSet::new(),
            revision: ValidationRevision::INITIAL,
            snapshot: None,
        }
    }

    #[cfg(test)]
    pub(crate) const fn open_documents(&self) -> &BTreeMap<PathBuf, OpenDocument> {
        &self.open_documents
    }

    pub(crate) fn build(&self) -> Option<&LspBuild> {
        self.snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.build.as_ref())
    }

    pub(crate) fn schema(&self) -> Option<&CftContainer> {
        self.build()
            .and_then(|build| build.schema.container.as_ref())
    }

    pub(crate) fn apply_open_document(
        &mut self,
        uri: String,
        text: String,
        version: Option<i64>,
    ) -> Result<bool, String> {
        let Some(path) = path_from_file_uri(&uri) else {
            return Ok(false);
        };
        self.open_documents
            .insert(normalize_path(&path), OpenDocument { uri, text, version });
        self.advance_revision()?;
        Ok(true)
    }

    pub(crate) fn apply_change_document(
        &mut self,
        uri: String,
        text: String,
        version: Option<i64>,
    ) -> Result<bool, String> {
        let Some(path) = path_from_file_uri(&uri) else {
            return Ok(false);
        };
        let normalized = normalize_path(&path);
        if self.open_documents.get(&normalized).is_some_and(|document| {
            matches!((version, document.version), (Some(next), Some(current)) if next <= current)
        }) {
            return Ok(false);
        }
        self.open_documents
            .entry(normalized)
            .and_modify(|document| {
                document.uri.clone_from(&uri);
                document.text.clone_from(&text);
                if version.is_some() {
                    document.version = version;
                }
            })
            .or_insert(OpenDocument { uri, text, version });
        self.advance_revision()?;
        Ok(true)
    }

    pub(crate) fn apply_close_document(&mut self, uri: &str) -> Result<bool, String> {
        let Some(path) = path_from_file_uri(uri) else {
            return Ok(false);
        };
        if self.open_documents.remove(&normalize_path(&path)).is_none() {
            return Ok(false);
        }
        self.advance_revision()?;
        Ok(true)
    }

    pub(crate) fn mark_project_changed(&mut self) -> Result<(), String> {
        self.advance_revision()
    }

    pub(crate) fn validation_input(&self) -> ValidationInput {
        ValidationInput::new(self.revision, &self.project, &self.open_documents)
    }

    pub(crate) fn is_current(&self) -> bool {
        self.snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.revision == self.revision)
    }

    pub(crate) fn commit_snapshot(
        &mut self,
        mut candidate: ValidationSnapshot,
    ) -> Vec<DiagnosticPublication> {
        if candidate.revision != self.revision {
            return Vec::new();
        }

        let mut publication_uris = self.published_uris.clone();
        publication_uris.extend(candidate.active_uris.iter().cloned());
        let publications = publication_uris
            .into_iter()
            .map(|uri| DiagnosticPublication {
                diagnostics: candidate.diagnostics.remove(&uri).unwrap_or_default(),
                version: candidate.document_versions.get(&uri).copied(),
                uri,
            })
            .collect();
        self.published_uris.clone_from(&candidate.active_uris);
        self.snapshot = Some(candidate);
        publications
    }

    pub(crate) fn open_document(
        &mut self,
        uri: String,
        text: String,
        version: Option<i64>,
    ) -> Result<Vec<DiagnosticPublication>, String> {
        if !self.apply_open_document(uri, text, version)? {
            return Ok(Vec::new());
        }
        Ok(self.validate_project())
    }

    pub(crate) fn change_document(
        &mut self,
        uri: String,
        text: String,
        version: Option<i64>,
    ) -> Result<Vec<DiagnosticPublication>, String> {
        if !self.apply_change_document(uri, text, version)? {
            return Ok(Vec::new());
        }
        Ok(self.validate_project())
    }

    pub(crate) fn close_document(
        &mut self,
        uri: &str,
    ) -> Result<Vec<DiagnosticPublication>, String> {
        if !self.apply_close_document(uri)? {
            return Ok(Vec::new());
        }
        Ok(self.validate_project())
    }

    pub(crate) fn refresh_project(&mut self) -> Result<Vec<DiagnosticPublication>, String> {
        self.mark_project_changed()?;
        Ok(self.validate_project())
    }

    pub(crate) fn validate_project(&mut self) -> Vec<DiagnosticPublication> {
        let input = self.validation_input();
        let candidate = build_snapshot(&input);
        self.commit_snapshot(candidate)
    }

    pub(crate) fn ensure_build_publications(&mut self) -> Vec<DiagnosticPublication> {
        if self
            .snapshot
            .as_ref()
            .is_none_or(|snapshot| snapshot.revision != self.revision)
        {
            return self.validate_project();
        }
        Vec::new()
    }

    pub(crate) fn prepare_request_document(&mut self, _uri: &str) -> Vec<DiagnosticPublication> {
        self.ensure_build_publications()
    }

    pub(crate) fn request_document(&self, uri: &str) -> LspRequestDocument<'_> {
        if let Some(source) = self.cfd_source_by_uri(uri) {
            let (ast, _) = parse_cfd(&source);
            return LspRequestDocument::Cfd(CfdRequestDocument {
                source,
                ast,
                schema: self.schema(),
                build: self.build(),
            });
        }
        let Some(build) = self.build() else {
            return LspRequestDocument::Missing;
        };
        let Some(document) = build.document_by_uri(uri) else {
            return LspRequestDocument::Missing;
        };
        LspRequestDocument::Cft { build, document }
    }

    fn cfd_source_by_uri(&self, uri: &str) -> Option<String> {
        let path = path_from_file_uri(uri)?;
        if !is_cfd_path(&path) {
            return None;
        }
        self.open_documents
            .get(&normalize_path(&path))
            .map(|document| document.text.clone())
    }

    fn advance_revision(&mut self) -> Result<(), String> {
        self.revision = self.revision.next()?;
        Ok(())
    }
}

pub(crate) fn is_cfd_path(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension == "cfd")
}
