use crate::validation::ProjectDiagnostic;
use coflow_api::{Diagnostic, DiagnosticSet, Label, Severity, SourceLocation};
use std::path::Path;

const PROJECT_DIAGNOSTIC_STAGE: &str = "PROJECT";

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
        contexts: Vec::new(),
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
        contexts: Vec::new(),
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
        contexts: Vec::new(),
    }
}
