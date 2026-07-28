use crate::uri::path_to_file_uri;
use coflow_project::normalize_path;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub fn lsp_diagnostic(diagnostic: &coflow_api::Diagnostic) -> Value {
    let related: Vec<_> = diagnostic
        .related
        .iter()
        .map(|related| {
            let location = lsp_label_location(&related.location);
            json!({
                "location": {
                    "uri": label_uri(&location, &BTreeMap::new()),
                    "range": lsp_range(
                        location.start_line,
                        location.start_character,
                        location.end_line,
                        location.end_character,
                    )
                },
                "message": related.message.as_deref().unwrap_or("")
            })
        })
        .collect();

    let primary = diagnostic
        .primary
        .as_ref()
        .map(|label| lsp_label_location(&label.location))
        .unwrap_or_default();
    let mut out = Map::new();
    out.insert(
        "range".to_string(),
        lsp_range(
            primary.start_line,
            primary.start_character,
            primary.end_line,
            primary.end_character,
        ),
    );
    out.insert(
        "severity".to_string(),
        json!(lsp_diagnostic_severity(diagnostic.severity)),
    );
    out.insert("code".to_string(), json!(&diagnostic.code));
    out.insert(
        "source".to_string(),
        json!(format!("coflow {}", diagnostic.stage)),
    );
    let mut message = diagnostic.message.clone();
    for context in &diagnostic.contexts {
        message.push_str("\n上下文: ");
        message.push_str(&context.human_message());
    }
    out.insert("message".to_string(), json!(message));

    if !related.is_empty() {
        out.insert("relatedInformation".to_string(), Value::Array(related));
    }

    Value::Object(out)
}

const fn lsp_diagnostic_severity(severity: coflow_api::Severity) -> u8 {
    match severity {
        coflow_api::Severity::Error => 1,
        coflow_api::Severity::Warning => 2,
        coflow_api::Severity::Info => 3,
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

pub fn lsp_label_location(location: &coflow_api::SourceLocation) -> LspLabelLocation {
    let range = location.text_range();
    match location {
        coflow_api::SourceLocation::FileSpan { path, .. }
        | coflow_api::SourceLocation::ProjectConfig { path, .. }
        | coflow_api::SourceLocation::Artifact { path }
        | coflow_api::SourceLocation::TableCell { path, .. } => LspLabelLocation {
            document: LspLabelDocument::Path(path.clone()),
            start_line: range.start.line,
            start_character: range.start.character,
            end_line: range.end.line,
            end_character: range.end.character,
        },
    }
}

pub fn lsp_error_diagnostic(code: &str, message: &str) -> Value {
    json!({
        "range": lsp_range(0, 0, 0, 1),
        "severity": 2,
        "code": code,
        "source": "cft LSP",
        "message": message
    })
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
) -> Value {
    json!({
        "start": {
            "line": start_line,
            "character": start_character
        },
        "end": {
            "line": end_line,
            "character": end_character
        }
    })
}
