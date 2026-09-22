use super::uri::path_to_file_uri;
use crate::service::{
    LanguageDiagnostic, LanguagePosition, LanguageRange, Location, RelatedInformation,
};
use coflow_project::normalize_path;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub fn lsp_diagnostic(diagnostic: &coflow_project::Diagnostic) -> LanguageDiagnostic {
    let related_information: Vec<_> = diagnostic
        .related
        .iter()
        .map(|related| {
            let location = lsp_label_location(&related.location);
            RelatedInformation {
                location: Location {
                    uri: label_uri(&location, &BTreeMap::new()),
                    range: lsp_range(
                        location.start_line,
                        location.start_character,
                        location.end_line,
                        location.end_character,
                    ),
                },
                message: related.message.clone().unwrap_or_default(),
            }
        })
        .collect();
    let primary = diagnostic
        .primary
        .as_ref()
        .map(|l| lsp_label_location(&l.location))
        .unwrap_or_default();
    let mut message = diagnostic.message.clone();
    for context in &diagnostic.contexts {
        message.push_str("\n上下文: ");
        message.push_str(&context.human_message());
    }
    LanguageDiagnostic {
        range: lsp_range(
            primary.start_line,
            primary.start_character,
            primary.end_line,
            primary.end_character,
        ),
        severity: lsp_diagnostic_severity(diagnostic.severity),
        code: Some(diagnostic.code.clone()),
        source: Some(format!("coflow {}", diagnostic.stage)),
        message,
        related_information: (!related_information.is_empty()).then_some(related_information),
    }
}

const fn lsp_diagnostic_severity(severity: coflow_project::Severity) -> u8 {
    match severity {
        coflow_project::Severity::Error => 1,
        coflow_project::Severity::Warning => 2,
        coflow_project::Severity::Info => 3,
    }
}

#[derive(Debug, Clone, Default)]
pub struct LspLabelLocation {
    document: LspLabelDocument,
    start_line: usize,
    start_character: usize,
    end_line: usize,
    end_character: usize,
}

#[derive(Debug, Clone, Default)]
enum LspLabelDocument {
    #[default]
    Unknown,
    Path(PathBuf),
}

pub fn lsp_label_location(location: &coflow_project::SourceLocation) -> LspLabelLocation {
    let range = location.text_range();
    match location {
        coflow_project::SourceLocation::FileSpan { path, .. }
        | coflow_project::SourceLocation::ProjectConfig { path, .. }
        | coflow_project::SourceLocation::Artifact { path } => LspLabelLocation {
            document: LspLabelDocument::Path(path.clone()),
            start_line: range.start.line,
            start_character: range.start.character,
            end_line: range.end.line,
            end_character: range.end.character,
        },
    }
}

pub fn lsp_error_diagnostic(code: &str, message: &str) -> LanguageDiagnostic {
    LanguageDiagnostic {
        range: lsp_range(0, 0, 0, 1),
        severity: 2,
        code: Some(code.into()),
        source: Some("cft LSP".into()),
        message: message.into(),
        ..Default::default()
    }
}

pub fn preferred_diagnostic_uri(preferred_uris: &BTreeMap<PathBuf, String>, path: &Path) -> String {
    preferred_uris
        .get(&normalize_path(path))
        .cloned()
        .unwrap_or_else(|| path_to_file_uri(path))
}

pub fn label_uri(
    location: &LspLabelLocation,
    preferred_uris: &BTreeMap<PathBuf, String>,
) -> String {
    match &location.document {
        LspLabelDocument::Path(path) => preferred_diagnostic_uri(preferred_uris, path),
        LspLabelDocument::Unknown => preferred_diagnostic_uri(preferred_uris, Path::new("")),
    }
}

pub fn lsp_range(
    start_line: usize,
    start_character: usize,
    end_line: usize,
    end_character: usize,
) -> LanguageRange {
    LanguageRange {
        start: LanguagePosition {
            line: start_line,
            character: start_character,
        },
        end: LanguagePosition {
            line: end_line,
            character: end_character,
        },
    }
}
