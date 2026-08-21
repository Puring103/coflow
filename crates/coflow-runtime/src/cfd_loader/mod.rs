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

use crate::api::{CfdLoadContext, CfdSource, Diagnostic, DiagnosticSet, LoadedCfdSource};

mod diagnostics;
mod lower;
pub mod writer;
use crate::data_model::{CfdDataModel, LoadedRecordDraft, RecordOrigin};
use coflow_language::cfd::parse_cfd;
use coflow_language::CftSchema;
use diagnostics::{cfd_error_to_diagnostics, text_span};
pub use diagnostics::{
    CfdTextDiagnostic, CfdTextDiagnostics, CfdTextErrorCode, CfdTextLoadError, CfdTextSpan,
};
use lower::{lower_records, syntax_diagnostics, ParsedLoadedRecordDraft};
use std::borrow::Cow;
use std::fs;
use std::path::{Path, PathBuf};
pub use writer::CfdWriter;

/// Parses `.cfd` text into source-neutral input records.
///
/// The returned records use the top-level CFD record name as
/// [`LoadedRecordDraft::key`]. No `id` field is emitted.
///
/// # Errors
///
/// Returns text diagnostics when parsing or schema-guided conversion fails.
pub fn parse_cfd_input_records(
    schema: &CftSchema,
    source: &str,
) -> Result<Vec<LoadedRecordDraft>, CfdTextLoadError> {
    parse_cfd_input_records_with_spans(schema, source).map(|records| {
        records
            .into_iter()
            .map(|record| record.record)
            .collect::<Vec<_>>()
    })
}

fn parse_cfd_input_records_with_spans(
    schema: &CftSchema,
    source: &str,
) -> Result<Vec<ParsedLoadedRecordDraft>, CfdTextLoadError> {
    let (ast, diagnostics) = parse_cfd(source);
    if !diagnostics.is_empty() {
        return Err(CfdTextLoadError::Text(syntax_diagnostics(diagnostics)));
    }
    lower_records(schema, &ast).map_err(CfdTextLoadError::Text)
}

/// Parses `.cfd` text and builds a validated [`CfdDataModel`].
///
/// # Errors
///
/// Returns text diagnostics for CFD syntax/conversion errors or data-model
/// diagnostics for schema/data/reference errors.
pub fn load_cfd_model(schema: &CftSchema, source: &str) -> Result<CfdDataModel, CfdTextLoadError> {
    let records = parse_cfd_input_records_with_spans(schema, source)?;
    let mut builder = CfdDataModel::builder(schema);
    let mut origins = Vec::with_capacity(records.len());
    for record in records {
        let origin = RecordOrigin::File {
            path: PathBuf::new(),
            span: Some(text_span(source, record.span)),
        };
        origins.push(origin.clone());
        builder.add_loaded_record(record.record.with_origin(origin));
    }
    builder
        .build()
        .map_err(|diagnostics| CfdTextLoadError::DataModel {
            diagnostics,
            origins,
        })
}

#[derive(Debug, Default, Clone, Copy)]
pub struct CfdLoader;

impl CfdLoader {
    pub fn resolve(&self, source: &CfdSource) -> Result<CfdSource, DiagnosticSet> {
        let path = source.location.path();
        if is_cfd_path(path) {
            return Ok(source.clone());
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

    pub fn load(
        &self,
        ctx: CfdLoadContext<'_>,
        source: &CfdSource,
    ) -> Result<LoadedCfdSource, DiagnosticSet> {
        let file = source.location.path();
        let contents = match ctx.source_text {
            Some(source) => Cow::Borrowed(source),
            None => Cow::Owned(fs::read_to_string(file).map_err(|err| {
                DiagnosticSet::one(Diagnostic::error(
                    "CFD-READ",
                    "CFD",
                    format!("failed to read CFD source `{}`: {err}", file.display()),
                ))
            })?),
        };
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
                LoadedCfdSource { records }
            })
            .map_err(|err| cfd_error_to_diagnostics(file, &contents, err))
    }
}

fn is_cfd_path(path: &Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()) == Some("cfd")
}
