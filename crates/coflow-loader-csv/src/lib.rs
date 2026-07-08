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
pub use format::{parse, write};
pub use source::{collect_input_records, CsvInputRecords, CsvSheet, CsvSource};
pub use writer::CsvWriter;

use coflow_api::{
    Diagnostic, DiagnosticSet, LoadedSource, ProbeResult, ProjectSourceRef, ResolvedSource,
    SourceLoadContext, SourceLocationSpec, SourceProvider, SourceProviderDescriptor,
    SourceResolveContext,
};
use options::csv_sheets_from_options;
use std::fs;
use std::path::Path;

#[derive(Debug, Default, Clone, Copy)]
pub struct CsvLoader;

pub const CSV_LOADER_DESCRIPTOR: SourceProviderDescriptor = SourceProviderDescriptor {
    id: "csv",
    display_name: "CSV file",
    extensions: &["csv"],
    uri_schemes: &[],
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
        if matches!(
            source.location,
            SourceLocationSpec::Path(path)
                if path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| CSV_LOADER_DESCRIPTOR.extensions.contains(&ext))
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
            if source.provider_id == CSV_LOADER_DESCRIPTOR.id {
                return Err(DiagnosticSet::one(Diagnostic::error(
                    "CSV-SOURCE",
                    "CSV",
                    "csv source requires `path`",
                )));
            }
            return Ok(Vec::new());
        };
        if path.is_dir() {
            return collect_csv_sources(path, source);
        }
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
        let SourceLocationSpec::Path(file) = &source.location else {
            return Err(DiagnosticSet::one(Diagnostic::error(
                "CSV-SOURCE",
                "CSV",
                "csv source requires `path`",
            )));
        };
        let sheets = csv_sheets_from_options(&source.options)?;
        let csv_source = CsvSource::new(file.clone(), sheets);
        collect_input_records(ctx.schema, &[csv_source])
            .map(|loaded| LoadedSource {
                records: loaded.records,
            })
            .map_err(csv_diagnostics_to_api)
    }
}

fn collect_csv_sources(
    dir: &Path,
    source: &ResolvedSource,
) -> Result<Vec<ResolvedSource>, DiagnosticSet> {
    let mut entries = fs::read_dir(dir)
        .map_err(|err| {
            DiagnosticSet::one(Diagnostic::error(
                "CSV-SOURCE",
                "CSV",
                format!("failed to read data source dir `{}`: {err}", dir.display()),
            ))
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| {
            DiagnosticSet::one(Diagnostic::error(
                "CSV-SOURCE",
                "CSV",
                format!("failed to read data source dir `{}`: {err}", dir.display()),
            ))
        })?;
    entries.sort_by_key(fs::DirEntry::path);

    let mut sources = Vec::new();
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            sources.extend(collect_csv_sources(&path, source)?);
        } else if is_csv_path(&path) {
            sources.push(ResolvedSource {
                provider_id: CSV_LOADER_DESCRIPTOR.id.to_string(),
                display_name: path.display().to_string(),
                location: SourceLocationSpec::Path(path),
                options: source.options.clone(),
            });
        }
    }
    Ok(sources)
}

fn is_csv_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| CSV_LOADER_DESCRIPTOR.extensions.contains(&ext))
}
