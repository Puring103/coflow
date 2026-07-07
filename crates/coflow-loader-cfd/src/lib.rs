//! Text `.cfd` loader for Coflow data models.

#![cfg_attr(
    not(test),
    deny(
        clippy::dbg_macro,
        clippy::expect_used,
        clippy::panic,
        clippy::panic_in_result_fn,
        clippy::todo,
        clippy::unimplemented,
        clippy::unreachable,
        clippy::unwrap_used
    )
)]
#![allow(clippy::missing_const_for_fn, clippy::similar_names, clippy::use_self)]

use coflow_api::{
    DataLoader, Diagnostic, DiagnosticSet, LoadContext, LoadedRecords, LoaderDescriptor,
    ProbeResult, ProjectSourceRef, RecordOrigin, ResolvedSource, SourceLocationSpec,
    SourceResolveContext,
};

mod diagnostics;
mod parser;
pub mod writer;
use coflow_cft::CftContainer;
use coflow_data_model::{CfdDataModel, CfdInputRecord};
use diagnostics::{cfd_error_to_diagnostics, text_span};
pub use diagnostics::{
    CfdTextDiagnostic, CfdTextDiagnostics, CfdTextErrorCode, CfdTextLoadError, CfdTextSpan,
};
use parser::{ParsedCfdInputRecord, Parser};
use std::fs;
use std::path::Path;
pub use writer::{CfdWriter, CFD_WRITER_DESCRIPTOR};

/// Parses `.cfd` text into source-neutral input records.
///
/// The returned records use the top-level CFD record name as
/// [`CfdInputRecord::key`]. No `id` field is emitted.
///
/// # Errors
///
/// Returns text diagnostics when parsing or schema-guided conversion fails.
pub fn parse_cfd_input_records(
    schema: &CftContainer,
    source: &str,
) -> Result<Vec<CfdInputRecord>, CfdTextLoadError> {
    parse_cfd_input_records_with_spans(schema, source).map(|records| {
        records
            .into_iter()
            .map(|record| record.record)
            .collect::<Vec<_>>()
    })
}

fn parse_cfd_input_records_with_spans(
    schema: &CftContainer,
    source: &str,
) -> Result<Vec<ParsedCfdInputRecord>, CfdTextLoadError> {
    Parser::new(schema, source)
        .parse_records_with_spans()
        .map_err(CfdTextLoadError::Text)
}

/// Parses `.cfd` text and builds a validated [`CfdDataModel`].
///
/// # Errors
///
/// Returns text diagnostics for CFD syntax/conversion errors or data-model
/// diagnostics for schema/data/reference errors.
pub fn load_cfd_model(
    schema: &CftContainer,
    source: &str,
) -> Result<CfdDataModel, CfdTextLoadError> {
    let records = parse_cfd_input_records(schema, source)?;
    let mut builder = CfdDataModel::builder(schema);
    for record in records {
        builder.add_input_record(record);
    }
    builder.build().map_err(CfdTextLoadError::DataModel)
}

#[derive(Debug, Default, Clone, Copy)]
pub struct CfdLoader;

pub const CFD_LOADER_DESCRIPTOR: LoaderDescriptor = LoaderDescriptor {
    id: "cfd",
    display_name: "Coflow data text",
    extensions: &["cfd"],
    uri_schemes: &[],
    option_keys: &[],
};

impl DataLoader for CfdLoader {
    fn descriptor(&self) -> &'static LoaderDescriptor {
        &CFD_LOADER_DESCRIPTOR
    }

    fn probe(&self, source: &ProjectSourceRef<'_>) -> ProbeResult {
        if source.source_type == Some(CFD_LOADER_DESCRIPTOR.id) {
            return ProbeResult::certain();
        }
        if matches!(
            source.location,
            SourceLocationSpec::Path(path)
                if path.extension().and_then(|ext| ext.to_str()) == Some("cfd")
        ) {
            ProbeResult::likely()
        } else {
            ProbeResult::none()
        }
    }

    fn resolve(
        &self,
        _ctx: SourceResolveContext<'_>,
        source: &ResolvedSource,
    ) -> Result<Vec<ResolvedSource>, DiagnosticSet> {
        let SourceLocationSpec::Path(path) = &source.location else {
            if source.provider_id == CFD_LOADER_DESCRIPTOR.id {
                return Err(DiagnosticSet::one(Diagnostic::error(
                    "CFD-SOURCE",
                    "CFD",
                    "cfd source requires `path`",
                )));
            }
            return Ok(Vec::new());
        };
        if path.is_dir() {
            return collect_cfd_sources(path, source);
        }
        if is_cfd_path(path) {
            if source.options.get("sheets").is_some() {
                return Err(DiagnosticSet::one(Diagnostic::error(
                    "CFD-SOURCE",
                    "CFD",
                    format!(
                        "CFD source `{}` cannot define `sheets`",
                        source.display_name
                    ),
                )));
            }
            return Ok(vec![source.clone()]);
        }
        Err(DiagnosticSet::one(Diagnostic::error(
            "CFD-SOURCE",
            "CFD",
            format!(
                "source file `{}` has unsupported extension",
                source.display_name
            ),
        )))
    }

    fn load(
        &self,
        ctx: LoadContext<'_>,
        source: &ResolvedSource,
    ) -> Result<LoadedRecords, DiagnosticSet> {
        let SourceLocationSpec::Path(file) = &source.location else {
            return Err(DiagnosticSet::one(Diagnostic::error(
                "CFD-SOURCE",
                "CFD",
                "cfd source requires `path`",
            )));
        };
        let contents = fs::read_to_string(file).map_err(|err| {
            DiagnosticSet::one(Diagnostic::error(
                "CFD-READ",
                "CFD",
                format!("failed to read CFD source `{}`: {err}", file.display()),
            ))
        })?;
        parse_cfd_input_records_with_spans(ctx.schema, &contents)
            .map(|records| {
                let records = records
                    .into_iter()
                    .map(|record| {
                        let span = text_span(&contents, record.span);
                        record.record.with_origin(RecordOrigin::File {
                            path: file.clone(),
                            span: Some(span),
                        })
                    })
                    .collect();
                LoadedRecords { records }
            })
            .map_err(|err| cfd_error_to_diagnostics(file, &contents, err))
    }
}

fn collect_cfd_sources(
    dir: &Path,
    source: &ResolvedSource,
) -> Result<Vec<ResolvedSource>, DiagnosticSet> {
    let mut entries = fs::read_dir(dir)
        .map_err(|err| {
            DiagnosticSet::one(Diagnostic::error(
                "CFD-SOURCE",
                "CFD",
                format!("failed to read data source dir `{}`: {err}", dir.display()),
            ))
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| {
            DiagnosticSet::one(Diagnostic::error(
                "CFD-SOURCE",
                "CFD",
                format!("failed to read data source dir `{}`: {err}", dir.display()),
            ))
        })?;
    entries.sort_by_key(fs::DirEntry::path);

    let mut sources = Vec::new();
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            sources.extend(collect_cfd_sources(&path, source)?);
        } else if is_cfd_path(&path) {
            sources.push(ResolvedSource {
                provider_id: CFD_LOADER_DESCRIPTOR.id.to_string(),
                display_name: path.display().to_string(),
                location: SourceLocationSpec::Path(path),
                options: source.options.clone(),
            });
        }
    }
    Ok(sources)
}

fn is_cfd_path(path: &Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()) == Some("cfd")
}
