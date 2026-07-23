use coflow_api::{
    Diagnostic, DiagnosticContext, DiagnosticSet, Label, Severity, SourceLocation,
};
use serde::Serialize;
use std::path::Path;

#[must_use]
pub fn cli_error(code: impl Into<String>, message: impl Into<String>) -> DiagnosticSet {
    DiagnosticSet::one(Diagnostic::error(code, "CLI", message))
}

#[must_use]
pub fn cli_file_error(
    path: &Path,
    code: impl Into<String>,
    message: impl Into<String>,
) -> DiagnosticSet {
    DiagnosticSet::one(Diagnostic::error(code, "CLI", message).with_primary(Label {
        location: SourceLocation::Artifact {
            path: path.to_path_buf(),
        },
        message: None,
    }))
}

#[derive(Debug, Serialize)]
pub struct DiagnosticJson {
    pub code: String,
    pub stage: String,
    pub severity: String,
    pub message: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sheet: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cell: Option<String>,
    #[serde(rename = "startLine")]
    pub start_line: usize,
    #[serde(rename = "startCharacter")]
    pub start_character: usize,
    #[serde(rename = "endLine")]
    pub end_line: usize,
    #[serde(rename = "endCharacter")]
    pub end_character: usize,
    pub related: Vec<RelatedJson>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub contexts: Vec<DiagnosticContext>,
}

#[derive(Debug, Serialize)]
pub struct RelatedJson {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sheet: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cell: Option<String>,
    #[serde(rename = "startLine")]
    pub start_line: usize,
    #[serde(rename = "startCharacter")]
    pub start_character: usize,
    #[serde(rename = "endLine")]
    pub end_line: usize,
    #[serde(rename = "endCharacter")]
    pub end_character: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[must_use]
pub fn diagnostic_json_from_set(diagnostics: DiagnosticSet) -> Vec<DiagnosticJson> {
    diagnostics
        .diagnostics
        .into_iter()
        .map(diagnostic_json_from_diagnostic)
        .collect()
}

fn diagnostic_json_from_diagnostic(diagnostic: Diagnostic) -> DiagnosticJson {
    let primary = diagnostic.primary.as_ref().map(label_location);
    DiagnosticJson {
        code: diagnostic.code,
        stage: diagnostic.stage,
        severity: severity_name(diagnostic.severity).to_string(),
        message: diagnostic.message,
        path: primary
            .as_ref()
            .map_or_else(String::new, |location| location.path.clone()),
        sheet: primary.as_ref().and_then(|location| location.sheet.clone()),
        cell: primary.as_ref().and_then(|location| location.cell.clone()),
        start_line: primary.as_ref().map_or(0, |location| location.start_line),
        start_character: primary
            .as_ref()
            .map_or(0, |location| location.start_character),
        end_line: primary.as_ref().map_or(0, |location| location.end_line),
        end_character: primary
            .as_ref()
            .map_or(1, |location| location.end_character),
        related: diagnostic
            .related
            .iter()
            .map(related_json_from_label)
            .collect(),
        contexts: diagnostic.contexts,
    }
}

fn related_json_from_label(label: &Label) -> RelatedJson {
    let location = label_location(label);
    RelatedJson {
        path: location.path,
        sheet: location.sheet,
        cell: location.cell,
        start_line: location.start_line,
        start_character: location.start_character,
        end_line: location.end_line,
        end_character: location.end_character,
        label: label.message.clone(),
    }
}

#[derive(Debug)]
struct JsonLocation {
    path: String,
    sheet: Option<String>,
    cell: Option<String>,
    start_line: usize,
    start_character: usize,
    end_line: usize,
    end_character: usize,
}

fn label_location(label: &Label) -> JsonLocation {
    let range = label.location.text_range();
    match &label.location {
        SourceLocation::FileSpan { path, .. } => JsonLocation {
            path: path.display().to_string(),
            sheet: None,
            cell: None,
            start_line: range.start.line,
            start_character: range.start.character,
            end_line: range.end.line,
            end_character: range.end.character,
        },
        SourceLocation::TableCell { path, .. } => JsonLocation {
            path: path.display().to_string(),
            sheet: label.location.sheet().map(str::to_string),
            cell: label.location.cell_name(),
            start_line: range.start.line,
            start_character: range.start.character,
            end_line: range.end.line,
            end_character: range.end.character,
        },
        SourceLocation::ProjectConfig { path, .. } | SourceLocation::Artifact { path } => {
            JsonLocation {
                path: path.display().to_string(),
                sheet: None,
                cell: None,
                start_line: range.start.line,
                start_character: range.start.character,
                end_line: range.end.line,
                end_character: range.end.character,
            }
        }
    }
}

const fn severity_name(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Info => "info",
    }
}
