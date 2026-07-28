//! CSV loader for Coflow data models.
//!
//! This crate owns the shared RFC 4180 parser/writer used by both the data
//! loader and localization CSV tables.

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
#![allow(clippy::missing_const_for_fn)]

mod diagnostics;
mod format;
mod options;
mod source;
pub mod writer;
pub use diagnostics::{
    csv_diagnostics_to_api, CsvDiagnostic, CsvDiagnostics, CsvLabel, CsvLocation,
};
pub use format::{parse, parse_delimited, write, write_delimited};
pub use source::{collect_input_records, CsvInputRecords, CsvSheet, CsvSource};
pub use writer::CsvWriter;

use coflow_api::{
    DecodedSourceOptions, Diagnostic, DiagnosticSet, LoadedSource, ProbeResult, ProjectSourceRef,
    ProviderBundle, ProviderRegistrationError, ResolvedSource, SourceLoadContext, SourceProvider,
    SourceProviderDescriptor, SourceResolveContext,
};
use options::{
    csv_sheets, csv_source_options, decode_csv_source_options, decode_tsv_source_options,
    tsv_source_options,
};
use serde_json::Value;
use std::path::Path;
use std::sync::Arc;

/// Declares every registry role implemented by the CSV provider package.
///
/// # Errors
///
/// Returns an error if two CSV implementations declare the same role id.
pub fn provider_bundle() -> Result<ProviderBundle, ProviderRegistrationError> {
    let writer = Arc::new(CsvWriter::new());
    let tsv_writer = Arc::new(CsvWriter::new_tsv());
    let mut bundle = ProviderBundle::default();
    bundle.add_source_provider(CsvLoader)?;
    bundle.add_source_provider(TsvLoader)?;
    bundle.add_source_writer_arc(Arc::clone(&writer))?;
    bundle.add_table_manager_arc(Arc::clone(&writer))?;
    bundle.add_dimension_source_manager_arc(writer)?;
    bundle.add_source_writer_arc(Arc::clone(&tsv_writer))?;
    bundle.add_table_manager_arc(Arc::clone(&tsv_writer))?;
    bundle.add_dimension_source_manager_arc(tsv_writer)?;
    Ok(bundle)
}

#[derive(Debug, Default, Clone, Copy)]
pub struct CsvLoader;

#[derive(Debug, Default, Clone, Copy)]
pub struct TsvLoader;

pub const CSV_LOADER_DESCRIPTOR: SourceProviderDescriptor = SourceProviderDescriptor {
    id: "csv",
    display_name: "CSV file",
    extensions: &["csv"],
    option_keys: &["sheets"],
};

pub const TSV_LOADER_DESCRIPTOR: SourceProviderDescriptor = SourceProviderDescriptor {
    id: "tsv",
    display_name: "TSV file",
    extensions: &["tsv"],
    option_keys: &["sheets"],
};

impl SourceProvider for CsvLoader {
    fn descriptor(&self) -> &'static SourceProviderDescriptor {
        &CSV_LOADER_DESCRIPTOR
    }

    fn probe(&self, source: &ProjectSourceRef<'_>) -> ProbeResult {
        if source.source_type == Some(CSV_LOADER_DESCRIPTOR.id) {
            return ProbeResult::certain();
        }
        if source
            .location
            .path()
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| CSV_LOADER_DESCRIPTOR.extensions.contains(&ext))
        {
            ProbeResult::likely()
        } else {
            ProbeResult::none()
        }
    }

    fn decode_options(&self, options: &Value) -> Result<DecodedSourceOptions, DiagnosticSet> {
        decode_csv_source_options(options)
    }

    fn resolve(
        &self,
        _ctx: SourceResolveContext<'_>,
        source: &ResolvedSource,
    ) -> Result<Vec<ResolvedSource>, DiagnosticSet> {
        let path = (&source.location).path();
        if is_csv_path(path) {
            let mut resolved = source.clone();
            resolved.provider_id = CSV_LOADER_DESCRIPTOR.id.to_string();
            return Ok(vec![resolved]);
        }
        Err(DiagnosticSet::one(Diagnostic::error(
            "CSV-SOURCE",
            "CSV",
            format!(
                "source file `{}` has unsupported extension",
                source.display_name
            ),
        )))
    }

    fn load(
        &self,
        ctx: SourceLoadContext<'_>,
        source: &ResolvedSource,
    ) -> Result<LoadedSource, DiagnosticSet> {
        let file = (&source.location).path();
        let sheets = csv_sheets(csv_source_options(source)?);
        let csv_source = CsvSource::new(file.clone(), sheets);
        collect_input_records(ctx.schema, &[csv_source])
            .map(|loaded| LoadedSource {
                records: loaded.records,
            })
            .map_err(csv_diagnostics_to_api)
    }
}

impl SourceProvider for TsvLoader {
    fn descriptor(&self) -> &'static SourceProviderDescriptor {
        &TSV_LOADER_DESCRIPTOR
    }

    fn probe(&self, source: &ProjectSourceRef<'_>) -> ProbeResult {
        if source.source_type == Some(TSV_LOADER_DESCRIPTOR.id) {
            return ProbeResult::certain();
        }
        if matches_extension(source.location.path(), TSV_LOADER_DESCRIPTOR.extensions) {
            ProbeResult::likely()
        } else {
            ProbeResult::none()
        }
    }

    fn decode_options(&self, options: &Value) -> Result<DecodedSourceOptions, DiagnosticSet> {
        decode_tsv_source_options(options)
    }

    fn resolve(
        &self,
        _ctx: SourceResolveContext<'_>,
        source: &ResolvedSource,
    ) -> Result<Vec<ResolvedSource>, DiagnosticSet> {
        if matches_extension(source.location.path(), TSV_LOADER_DESCRIPTOR.extensions) {
            let mut resolved = source.clone();
            resolved.provider_id = TSV_LOADER_DESCRIPTOR.id.to_string();
            return Ok(vec![resolved]);
        }
        Err(DiagnosticSet::one(Diagnostic::error(
            "TSV-SOURCE",
            "TSV",
            format!(
                "source file `{}` has unsupported extension",
                source.display_name
            ),
        )))
    }

    fn load(
        &self,
        ctx: SourceLoadContext<'_>,
        source: &ResolvedSource,
    ) -> Result<LoadedSource, DiagnosticSet> {
        let sheets = csv_sheets(tsv_source_options(source)?);
        let tsv_source =
            CsvSource::new(source.location.path().clone(), sheets).with_delimiter('\t');
        collect_input_records(ctx.schema, &[tsv_source])
            .map(|loaded| LoadedSource {
                records: loaded.records,
            })
            .map_err(csv_diagnostics_to_api)
    }
}

fn is_csv_path(path: &Path) -> bool {
    matches_extension(path, CSV_LOADER_DESCRIPTOR.extensions)
}

fn matches_extension(path: &Path, extensions: &[&str]) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| extensions.contains(&ext))
}
