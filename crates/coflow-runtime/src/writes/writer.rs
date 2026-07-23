use std::sync::Arc;

use coflow_api::{
    Diagnostic, DiagnosticSet, ProviderRegistry, ResolvedSource, Severity, SourceWriter,
};

use crate::indexes::SourceId;
use crate::ProjectSession;

pub(super) fn source_for_id(
    session: &ProjectSession,
    source_id: SourceId,
) -> Result<ResolvedSource, DiagnosticSet> {
    session
        .sources
        .get(source_id)
        .map(|entry| entry.source.clone())
        .ok_or_else(|| {
            DiagnosticSet::one(Diagnostic::error(
                "WRITE-NO-SOURCE",
                "WRITE",
                format!(
                    "no resolved source recorded for source id {} (cannot dispatch write)",
                    source_id.index()
                ),
            ))
        })
}

pub(super) fn source_for_file(
    session: &ProjectSession,
    file: &str,
) -> Result<ResolvedSource, DiagnosticSet> {
    match session.files.sources_for_display(file) {
        [source_id] => source_for_id(session, *source_id),
        [] => Err(DiagnosticSet::one(Diagnostic::error(
            "WRITE-NO-SOURCE",
            "WRITE",
            format!("no resolved source recorded for file `{file}` (cannot dispatch write)"),
        ))),
        sources => Err(DiagnosticSet::one(Diagnostic::error(
            "WRITE-AMBIGUOUS-SOURCE",
            "WRITE",
            format!(
                "file `{file}` resolves to {} configured sources; address an existing record or remove the duplicate source configuration",
                sources.len()
            ),
        ))),
    }
}

pub(super) fn lookup_source_writer(
    registry: &ProviderRegistry,
    source: &ResolvedSource,
) -> Result<Arc<dyn SourceWriter>, DiagnosticSet> {
    registry.source_writer(&source.provider_id).ok_or_else(|| {
        DiagnosticSet::one(Diagnostic {
            code: "WRITE-NO-WRITER".to_string(),
            stage: "WRITE".to_string(),
            severity: Severity::Error,
            message: format!("no writer registered for provider `{}`", source.provider_id),
            primary: None,
            related: Vec::new(),
            contexts: Vec::new(),
        })
    })
}
