use std::sync::Arc;

use coflow_api::{
    Diagnostic, DiagnosticSet, ProviderRegistry, ResolvedSource, Severity, SourceWriter,
};

use crate::ProjectSession;

pub(super) fn source_for_file(
    session: &ProjectSession,
    file: &str,
) -> Result<ResolvedSource, DiagnosticSet> {
    session
        .files
        .source_for_display(file)
        .and_then(|source_id| session.sources.entries().get(source_id.index()))
        .map(|entry| entry.source.clone())
        .ok_or_else(|| {
            DiagnosticSet::one(Diagnostic::error(
                "WRITE-NO-SOURCE",
                "WRITE",
                format!("no resolved source recorded for file `{file}` (cannot dispatch write)"),
            ))
        })
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
        })
    })
}
