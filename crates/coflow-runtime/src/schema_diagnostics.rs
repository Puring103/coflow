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
        contexts: Vec::new(),
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

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::{dedupe_cft_diagnostics, diagnostic_set_from_cft};
    use coflow_api::SourceLocation;
    use coflow_cft::{CftDiagnostic, CftErrorCode, CftSeverity, ModuleId, Span};
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    #[test]
    fn canonical_conversion_deduplicates_complete_diagnostic_identity() {
        let source = "type 表 {\n  名: string;\n}\n";
        let start = source.find('名').expect("field name");
        let end = start + "名".len();
        let diagnostic = CftDiagnostic::error(
            CftErrorCode::UnknownNamedType,
            ModuleId::new("schema/main.cft"),
            Span::new(start, end),
            "bad type",
        )
        .with_primary_message("primary")
        .with_related(
            ModuleId::new("schema/other.cft"),
            Span::new(0, 4),
            "related",
        );
        let distinct = diagnostic.clone().with_related(
            ModuleId::new("schema/other.cft"),
            Span::new(5, 9),
            "related",
        );
        let diagnostics =
            dedupe_cft_diagnostics(vec![diagnostic.clone(), diagnostic.clone(), distinct]);
        assert_eq!(diagnostics.len(), 2);

        let sources = BTreeMap::from([
            ("schema/main.cft".to_string(), source.to_string()),
            ("schema/other.cft".to_string(), "enum E {}".to_string()),
        ]);
        let paths = BTreeMap::from([
            (
                "schema/main.cft".to_string(),
                "C:/project/schema/main.cft".to_string(),
            ),
            (
                "schema/other.cft".to_string(),
                "C:/project/schema/other.cft".to_string(),
            ),
        ]);
        let converted = diagnostic_set_from_cft(vec![diagnostic], &sources, &paths);
        let converted = converted.diagnostics.first().expect("canonical diagnostic");
        assert!(matches!(
            converted.primary.as_ref().map(|label| &label.location),
            Some(SourceLocation::FileSpan {
                path,
                start_line: 1,
                start_character: 2,
                end_line: 1,
                end_character: 3,
            }) if path == &PathBuf::from("C:/project/schema/main.cft")
        ));
        assert!(matches!(
            converted.related.first(),
            Some(label) if label.message.as_deref() == Some("related")
                && matches!(&label.location, SourceLocation::FileSpan { path, .. }
                    if path == &PathBuf::from("C:/project/schema/other.cft"))
        ));
    }

    #[test]
    fn deduplication_supports_diagnostics_without_primary_labels() {
        let diagnostic = CftDiagnostic {
            code: CftErrorCode::UnexpectedEof,
            stage: CftErrorCode::UnexpectedEof.stage(),
            severity: CftSeverity::Error,
            message: "missing token".to_string(),
            primary: None,
            related: Vec::new(),
        };
        let deduped = dedupe_cft_diagnostics(vec![diagnostic.clone(), diagnostic]);
        assert_eq!(deduped.len(), 1);
        assert!(deduped[0].primary.is_none());
    }
}
