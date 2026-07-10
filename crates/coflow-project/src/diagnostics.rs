use crate::validation::ProjectDiagnostic;
use coflow_api::{
    byte_range, Diagnostic, DiagnosticSet, Label, Severity, SourceLocation, TextRange,
};
use coflow_cft::{CftDiagnostic, CftLabel};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

const PROJECT_DIAGNOSTIC_STAGE: &str = "PROJECT";

#[must_use]
pub fn dedupe_cft_diagnostics(diagnostics: Vec<CftDiagnostic>) -> Vec<CftDiagnostic> {
    let mut keys = BTreeSet::new();
    let mut out = Vec::new();
    for diagnostic in diagnostics {
        if keys.insert(cft_diagnostic_key(&diagnostic)) {
            out.push(diagnostic);
        }
    }
    out
}

fn cft_diagnostic_key(diagnostic: &CftDiagnostic) -> String {
    let mut key = format!(
        "{}\n{}\n{}\n",
        diagnostic.code.as_str(),
        diagnostic.stage,
        diagnostic.message
    );
    if let Some(primary) = &diagnostic.primary {
        push_cft_label_key(&mut key, primary);
    }
    for related in &diagnostic.related {
        push_cft_label_key(&mut key, related);
    }
    key
}

fn push_cft_label_key(key: &mut String, label: &CftLabel) {
    key.push_str(label.module.as_str());
    key.push(':');
    key.push_str(&label.span.start.to_string());
    key.push(':');
    key.push_str(&label.span.end.to_string());
    key.push(':');
    if let Some(message) = &label.message {
        key.push_str(message);
    }
    key.push('\n');
}

#[must_use]
pub fn diagnostic_set_from_cft(
    diagnostics: Vec<CftDiagnostic>,
    sources: &BTreeMap<String, String>,
    paths: &BTreeMap<String, String>,
) -> DiagnosticSet {
    DiagnosticSet {
        diagnostics: diagnostics
            .into_iter()
            .map(|diagnostic| diagnostic_from_cft(diagnostic, sources, paths))
            .collect(),
    }
}

fn diagnostic_from_cft(
    diagnostic: CftDiagnostic,
    sources: &BTreeMap<String, String>,
    paths: &BTreeMap<String, String>,
) -> Diagnostic {
    Diagnostic {
        code: diagnostic.code.as_str().to_string(),
        stage: diagnostic.stage.to_string(),
        severity: Severity::Error,
        message: diagnostic.message,
        primary: diagnostic
            .primary
            .as_ref()
            .map(|label| label_from_cft(label, sources, paths)),
        related: diagnostic
            .related
            .iter()
            .map(|label| label_from_cft(label, sources, paths))
            .collect(),
    }
}

fn label_from_cft(
    label: &CftLabel,
    sources: &BTreeMap<String, String>,
    paths: &BTreeMap<String, String>,
) -> Label {
    let range = cft_label_range(label, sources);
    let path = paths
        .get(label.module.as_str())
        .map_or_else(|| PathBuf::from(label.module.as_str()), PathBuf::from);
    Label {
        location: SourceLocation::FileSpan {
            path,
            start_line: range.start.line,
            start_character: range.start.character,
            end_line: range.end.line,
            end_character: range.end.character,
        },
        message: label.message.clone(),
    }
}

fn cft_label_range(label: &CftLabel, sources: &BTreeMap<String, String>) -> TextRange {
    let source = sources
        .get(label.module.as_str())
        .map_or("", String::as_str);
    byte_range(source, label.span.start, label.span.end)
}

pub(super) fn project_diagnostics_to_set(
    config_path: &Path,
    diagnostics: Vec<ProjectDiagnostic>,
) -> DiagnosticSet {
    DiagnosticSet {
        diagnostics: diagnostics
            .into_iter()
            .map(|diagnostic| project_diagnostic(config_path, diagnostic))
            .collect(),
    }
}

#[must_use]
pub fn plain_error(
    code: impl Into<String>,
    stage: impl Into<String>,
    message: impl Into<String>,
) -> DiagnosticSet {
    DiagnosticSet::one(Diagnostic {
        code: code.into(),
        stage: stage.into(),
        severity: Severity::Error,
        message: message.into(),
        primary: None,
        related: Vec::new(),
    })
}

#[must_use]
pub fn file_error(
    path: &Path,
    code: impl Into<String>,
    stage: impl Into<String>,
    message: impl Into<String>,
) -> DiagnosticSet {
    DiagnosticSet::one(Diagnostic {
        code: code.into(),
        stage: stage.into(),
        severity: Severity::Error,
        message: message.into(),
        primary: Some(Label {
            location: SourceLocation::FileSpan {
                path: path.to_path_buf(),
                start_line: 0,
                start_character: 0,
                end_line: 0,
                end_character: 1,
            },
            message: None,
        }),
        related: Vec::new(),
    })
}

fn project_diagnostic(config_path: &Path, diagnostic: ProjectDiagnostic) -> Diagnostic {
    Diagnostic {
        code: diagnostic.code.unwrap_or_else(|| "PROJECT-001".to_string()),
        stage: PROJECT_DIAGNOSTIC_STAGE.to_string(),
        severity: Severity::Error,
        message: diagnostic.message,
        primary: Some(Label {
            location: SourceLocation::ProjectConfig {
                path: config_path.to_path_buf(),
                key_path: diagnostic.key_path,
            },
            message: None,
        }),
        related: Vec::new(),
    }
}

