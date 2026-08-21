//! Source origin metadata for records.
//!
//! Each top-level [`crate::data_model::CfdRecord`] carries a [`RecordOrigin`] that
//! identifies where the record came from. The origin is the single source of
//! truth used by:
//! - diagnostics: to map record-anchored errors back to CFD text locations,
//! - writers: to dispatch edits back to the owning CFD document.
//!
//! Origins are source-neutral: they describe *where* a record lives, not
//! *which loader* produced it. Loaders set the appropriate variant when they
//! parse records; writers branch on the variant to perform an edit.
use crate::data_model::diagnostics::{CfdDiagnostic, CfdDiagnostics, CfdLabel, CfdPath};
use crate::data_model::model::CfdRecordId;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Where a record originated.
///
/// `None` is used for tests, ad-hoc model construction, and intermediate
/// values that aren't backed by a source location yet. Diagnostics and writers
/// must handle `None` gracefully (typically by treating the record as
/// non-editable / unlocatable).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum RecordOrigin {
    /// No known origin.
    #[default]
    None,
    /// Record came from a text file with a byte/line span.
    File {
        path: PathBuf,
        span: Option<TextSpan>,
    },
}

impl RecordOrigin {
    /// True when the origin is a local file path.
    #[must_use]
    pub fn is_file(&self) -> bool {
        matches!(self, Self::File { .. })
    }

    /// Return the source document path/label when one is associated with the origin.
    #[must_use]
    pub fn document_label(&self) -> Option<String> {
        match self {
            Self::None => None,
            Self::File { path, .. } => Some(path.display().to_string()),
        }
    }

    /// Return the local file path if this origin references one.
    #[must_use]
    pub fn local_path(&self) -> Option<&PathBuf> {
        match self {
            Self::File { path, .. } => Some(path),
            Self::None => None,
        }
    }

    /// Resolve a record-anchored path inside this origin to a concrete location.
    #[must_use]
    pub fn location_for_path(&self, _path: &CfdPath) -> Option<SourceLocation> {
        match self {
            Self::None => None,
            Self::File {
                path: file_path,
                span,
            } => {
                let span = span.unwrap_or(TextSpan {
                    start_line: 0,
                    start_character: 0,
                    end_line: 0,
                    end_character: 1,
                });
                Some(SourceLocation::FileSpan {
                    path: file_path.clone(),
                    start_line: span.start_line,
                    start_character: span.start_character,
                    end_line: span.end_line,
                    end_character: span.end_character,
                })
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextSpan {
    pub start_line: usize,
    pub start_character: usize,
    pub end_line: usize,
    pub end_character: usize,
}

/// Concrete location of a label inside a source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SourceLocation {
    FileSpan {
        path: PathBuf,
        start_line: usize,
        start_character: usize,
        end_line: usize,
        end_character: usize,
    },
}

/// Map a [`CfdLabel`] anchored on a record id to a [`SourceLocation`] using the
/// record's own origin from the model.
#[must_use]
pub fn label_to_location(label: &CfdLabel, origin: &RecordOrigin) -> Option<MappedLabel> {
    let location = origin.location_for_path(&label.path)?;
    Some(MappedLabel {
        location,
        message: label.message.clone(),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MappedLabel {
    pub location: SourceLocation,
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MappedDiagnostic {
    pub code: String,
    pub stage: String,
    pub message: String,
    pub primary: Option<MappedLabel>,
    pub related: Vec<MappedLabel>,
    pub source: CfdDiagnostic,
}

/// Map a set of [`CfdDiagnostics`] using a function that resolves a
/// [`CfdRecordId`] to its origin. Records without an origin yield a label-less
/// mapped diagnostic so callers still see the message.
#[must_use]
pub fn map_diagnostics(
    diagnostics: CfdDiagnostics,
    resolve: impl Fn(CfdRecordId) -> Option<RecordOrigin>,
) -> Vec<MappedDiagnostic> {
    diagnostics
        .diagnostics
        .into_iter()
        .map(|diagnostic| {
            let primary = diagnostic.primary.as_ref().and_then(|label| {
                let origin = match &label.origin {
                    Some(origin) => origin.clone(),
                    None => resolve(label.record?)?,
                };
                label_to_location(label, &origin)
            });
            let related = diagnostic
                .related
                .iter()
                .filter_map(|label| {
                    let origin = match &label.origin {
                        Some(origin) => origin.clone(),
                        None => resolve(label.record?)?,
                    };
                    label_to_location(label, &origin)
                })
                .collect();
            MappedDiagnostic {
                code: diagnostic.code.as_str().to_string(),
                stage: diagnostic.stage.to_string(),
                message: diagnostic.message.clone(),
                primary,
                related,
                source: diagnostic,
            }
        })
        .collect()
}
