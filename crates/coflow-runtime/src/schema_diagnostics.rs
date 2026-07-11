use coflow_api::{
    byte_range, Diagnostic, DiagnosticSet, Label, Severity, SourceLocation, TextRange,
};
use coflow_cft::{CftDiagnostic, CftLabel};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

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
