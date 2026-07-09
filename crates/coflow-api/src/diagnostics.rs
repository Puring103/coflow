use crate::{CfdDiagnostics, CfdInputRecord, RecordOrigin};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticSet {
    pub diagnostics: Vec<Diagnostic>,
}

impl DiagnosticSet {
    #[must_use]
    pub fn empty() -> Self {
        Self {
            diagnostics: Vec::new(),
        }
    }

    #[must_use]
    pub fn one(diagnostic: Diagnostic) -> Self {
        Self {
            diagnostics: vec![diagnostic],
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.diagnostics.is_empty()
    }

    pub fn iter(&self) -> std::slice::Iter<'_, Diagnostic> {
        self.diagnostics.iter()
    }

    pub fn extend(&mut self, other: Self) {
        self.diagnostics.extend(other.diagnostics);
    }

    pub fn push(&mut self, diagnostic: Diagnostic) {
        self.diagnostics.push(diagnostic);
    }

    #[must_use]
    pub fn contains(&self, needle: &str) -> bool {
        self.to_string().contains(needle)
    }
}

impl fmt::Display for DiagnosticSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, diagnostic) in self.diagnostics.iter().enumerate() {
            if index > 0 {
                writeln!(f)?;
            }
            write!(
                f,
                "[{}] [{}] {}",
                diagnostic.code, diagnostic.stage, diagnostic.message
            )?;
        }
        Ok(())
    }
}

impl From<Vec<Diagnostic>> for DiagnosticSet {
    fn from(diagnostics: Vec<Diagnostic>) -> Self {
        Self { diagnostics }
    }
}

impl<'a> IntoIterator for &'a DiagnosticSet {
    type Item = &'a Diagnostic;
    type IntoIter = std::slice::Iter<'a, Diagnostic>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub code: String,
    pub stage: String,
    pub severity: Severity,
    pub message: String,
    pub primary: Option<Label>,
    #[serde(default)]
    pub related: Vec<Label>,
}

impl Diagnostic {
    #[must_use]
    pub fn error(
        code: impl Into<String>,
        stage: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code: code.into(),
            stage: stage.into(),
            severity: Severity::Error,
            message: message.into(),
            primary: None,
            related: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_primary(mut self, label: Label) -> Self {
        self.primary = Some(label);
        self
    }

    /// Flatten a diagnostic into the wire shape consumed by editor hosts.
    /// `actual_type` / `record_key` / `field_path` are not derivable from the structured
    /// diagnostic alone; hosts that know the record id of the diagnostic's
    /// label populate them out-of-band.
    #[must_use]
    pub fn flat_view(
        &self,
        actual_type: Option<String>,
        record_key: Option<String>,
        field_path: Option<String>,
    ) -> FlatDiagnostic {
        let file_path = self
            .primary
            .as_ref()
            .map(|label| source_location_display_path(&label.location));
        FlatDiagnostic {
            severity: severity_str(self.severity).to_string(),
            code: self.code.clone(),
            stage: self.stage.clone(),
            message: self.message.clone(),
            file_path,
            actual_type,
            record_key,
            field_path,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Label {
    pub location: SourceLocation,
    pub message: Option<String>,
}

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
    TableCell {
        path: PathBuf,
        sheet: Option<String>,
        row: usize,
        column: usize,
    },
    RemoteCell {
        document: String,
        sheet: Option<String>,
        row: usize,
        column: usize,
    },
    ProjectConfig {
        path: PathBuf,
        key_path: Vec<String>,
    },
    Artifact {
        path: PathBuf,
    },
}

impl From<coflow_data_model::SourceLocation> for SourceLocation {
    fn from(loc: coflow_data_model::SourceLocation) -> Self {
        match loc {
            coflow_data_model::SourceLocation::FileSpan {
                path,
                start_line,
                start_character,
                end_line,
                end_character,
            } => Self::FileSpan {
                path,
                start_line,
                start_character,
                end_line,
                end_character,
            },
            coflow_data_model::SourceLocation::TableCell {
                path,
                sheet,
                row,
                column,
            } => Self::TableCell {
                path,
                sheet,
                row,
                column,
            },
            coflow_data_model::SourceLocation::RemoteCell {
                document,
                sheet,
                row,
                column,
            } => Self::RemoteCell {
                document,
                sheet,
                row,
                column,
            },
        }
    }
}

/// Map [`CfdDiagnostics`] to [`DiagnosticSet`] using a record-to-origin lookup.
///
/// Loaders no longer maintain a separate [`coflow_data_model::origin::RecordOrigin`]
/// map: each [`CfdInputRecord`] carries its own origin. Callers that need to
/// produce wire diagnostics from compiler/check failures pass either a slice
/// of records (or their extracted origins) and let this helper resolve labels.
#[must_use]
pub fn map_diagnostics_with_origins(
    diagnostics: CfdDiagnostics,
    origins: &[RecordOrigin],
) -> DiagnosticSet {
    let mapped =
        coflow_data_model::map_diagnostics(diagnostics, |id| origins.get(id.index()).cloned());
    DiagnosticSet {
        diagnostics: mapped
            .into_iter()
            .map(|d| Diagnostic {
                code: d.code,
                stage: d.stage,
                severity: Severity::Error,
                message: d.message,
                primary: d.primary.map(|l| Label {
                    location: l.location.into(),
                    message: l.message,
                }),
                related: d
                    .related
                    .into_iter()
                    .map(|l| Label {
                        location: l.location.into(),
                        message: l.message,
                    })
                    .collect(),
            })
            .collect(),
    }
}

/// Convenience helper: extract origins from a slice of input records.
#[must_use]
pub fn origins_of(records: &[CfdInputRecord]) -> Vec<RecordOrigin> {
    records.iter().map(|r| r.origin.clone()).collect()
}

/// Wire-friendly flat view of a [`Diagnostic`].
///
/// Editor hosts use this as a single severity/code/message tuple anchored to
/// a file/record/field. Heavier-weight callers can keep the structured form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
pub struct FlatDiagnostic {
    pub severity: String,
    pub code: String,
    pub stage: String,
    pub message: String,
    pub file_path: Option<String>,
    pub actual_type: Option<String>,
    pub record_key: Option<String>,
    pub field_path: Option<String>,
}

fn severity_str(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Info => "info",
    }
}

fn source_location_display_path(location: &SourceLocation) -> String {
    let path_to_slash = |path: &Path| path.to_string_lossy().replace('\\', "/");
    match location {
        SourceLocation::FileSpan { path, .. }
        | SourceLocation::TableCell { path, .. }
        | SourceLocation::ProjectConfig { path, .. }
        | SourceLocation::Artifact { path } => path_to_slash(path),
        SourceLocation::RemoteCell { document, .. } => document.clone(),
    }
}
