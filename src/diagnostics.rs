use coflow_api::{
    byte_range, Diagnostic, DiagnosticSet, Label, Severity, SourceLocation, TextRange,
};
use coflow_cft::{CftDiagnostic, CftLabel, ModuleId, Span};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::Path;

const PROJECT_DIAGNOSTIC_STAGE: &str = "PROJECT";

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

#[must_use]
pub fn diagnostic_messages(diagnostics: &DiagnosticSet) -> String {
    diagnostics
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.message.as_str())
        .collect::<Vec<_>>()
        .join("\n")
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
}

impl DiagnosticJson {
    #[must_use]
    pub fn project(message: impl Into<String>) -> Self {
        Self::plain("PROJECT-001", PROJECT_DIAGNOSTIC_STAGE, message)
    }

    #[must_use]
    pub fn artifact(message: impl Into<String>) -> Self {
        Self::plain("ARTIFACT-001", "ARTIFACT", message)
    }

    #[must_use]
    pub fn codegen(
        code: impl Into<String>,
        stage: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self::plain(code, stage, message)
    }

    fn plain(
        code: impl Into<String>,
        stage: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code: code.into(),
            stage: stage.into(),
            severity: "error".to_string(),
            message: message.into(),
            path: String::new(),
            sheet: None,
            cell: None,
            start_line: 0,
            start_character: 0,
            end_line: 0,
            end_character: 1,
            related: Vec::new(),
        }
    }

    #[must_use]
    pub fn from_cft(
        diagnostic: &CftDiagnostic,
        sources: &BTreeMap<String, String>,
        paths: &BTreeMap<String, String>,
    ) -> Self {
        let fallback = CftLabel {
            module: ModuleId::new(""),
            span: Span::default(),
            message: None,
        };
        let primary = diagnostic.primary.as_ref().unwrap_or(&fallback);
        let range = cft_label_range(primary, sources);
        let path = paths
            .get(primary.module.as_str())
            .map_or_else(|| primary.module.as_str().to_string(), Clone::clone);
        Self {
            code: diagnostic.code.as_str().to_string(),
            stage: diagnostic.stage.to_string(),
            severity: "error".to_string(),
            message: diagnostic.message.clone(),
            path,
            sheet: None,
            cell: None,
            start_line: range.start.line,
            start_character: range.start.character,
            end_line: range.end.line,
            end_character: range.end.character,
            related: diagnostic
                .related
                .iter()
                .map(|label| RelatedJson::from_cft(label, sources, paths))
                .collect(),
        }
    }
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

impl RelatedJson {
    fn from_cft(
        label: &CftLabel,
        sources: &BTreeMap<String, String>,
        paths: &BTreeMap<String, String>,
    ) -> Self {
        let range = cft_label_range(label, sources);
        let path = paths
            .get(label.module.as_str())
            .map_or_else(|| label.module.as_str().to_string(), Clone::clone);
        Self {
            path,
            sheet: None,
            cell: None,
            start_line: range.start.line,
            start_character: range.start.character,
            end_line: range.end.line,
            end_character: range.end.character,
            label: label.message.clone(),
        }
    }
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
        SourceLocation::RemoteCell { .. } => JsonLocation {
            path: label.location.display_path(),
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

fn cft_label_range(label: &CftLabel, sources: &BTreeMap<String, String>) -> TextRange {
    let source = sources
        .get(label.module.as_str())
        .map_or("", String::as_str);
    byte_range(source, label.span.start, label.span.end)
}
