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

pub(super) fn dimension_source_for_file(
    session: &ProjectSession,
    file: &str,
) -> Result<CfdSource, DiagnosticSet> {
    if !session.files.sources_for_display(file).is_empty() {
        return source_for_file(session, file);
    }
    let path = crate::resolve_existing_or_future_path(
        &session.project.resolve_path(std::path::Path::new(file)),
    )
    .map_err(|error| {
        DiagnosticSet::one(Diagnostic::error(
            "WRITE-NO-SOURCE",
            "WRITE",
            error.to_string(),
        ))
    })?;
    if path.extension().and_then(|extension| extension.to_str()) != Some("cfd") {
        return Err(DiagnosticSet::one(Diagnostic::error(
            "WRITE-NO-SOURCE",
            "WRITE",
            "dimension destinations must be CFD files",
        )));
    }
    Ok(CfdSource {
        display_name: crate::project_path(session.project.root_dir(), &path),
        location: crate::api::CfdSourcePath::new(path),
    })
}

pub(super) fn lookup_source_writer(catalog: &CfdSourceCatalog) -> Arc<CfdWriter> {
    catalog.writer()
}
