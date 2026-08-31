use std::sync::Arc;

use crate::api::{CfdSource, CfdSourceCatalog, Diagnostic, DiagnosticSet};
use crate::cfd_loader::CfdWriter;

use crate::indexes::SourceId;
use crate::ProjectSession;

pub(super) fn source_for_id(
    session: &ProjectSession,
    source_id: SourceId,
) -> Result<CfdSource, DiagnosticSet> {
    session
        .sources()
        .entries()
        .get(source_id.index())
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
) -> Result<CfdSource, DiagnosticSet> {
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

pub(super) fn lookup_source_writer(catalog: &CfdSourceCatalog) -> Arc<CfdWriter> {
    catalog.writer()
}
