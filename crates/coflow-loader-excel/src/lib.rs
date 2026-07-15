//! Excel `.xlsx` loader for Coflow data models.
//!
//! This crate deliberately accepts already-parsed loader configuration. YAML,
//! JSON, editor settings, and command-line parsing should live in higher layers.

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
#![allow(clippy::missing_const_for_fn, clippy::multiple_crate_versions)]

use coflow_api::{
    DecodedSourceOptions, Diagnostic, DiagnosticSet, LoadedSource, ProbeResult, ProjectSourceRef,
    ProviderBundle, ProviderRegistrationError, ResolvedSource, SourceLoadContext,
    SourceLocationSpec, SourceProvider, SourceProviderDescriptor, SourceResolveContext,
};
use std::fs;
use std::path::Path;
use std::sync::Arc;

mod diagnostics;
mod options;
mod source;
pub mod writer;
use diagnostics::excel_diagnostics_to_api;
pub use diagnostics::{
    map_label_with_record_offset, ExcelDiagnostic, ExcelDiagnostics, ExcelLabel, ExcelLocation,
};
use options::{decode_excel_source_options, excel_sheets, excel_source_options};
use serde_json::Value;
pub use source::{collect_input_records, ExcelInputRecords, ExcelSheet, ExcelSource};
pub use writer::{ExcelWriter, EXCEL_WRITER_DESCRIPTOR};

/// Declares every registry role implemented by the Excel provider package.
///
/// # Errors
///
/// Returns an error if two Excel implementations declare the same role id.
pub fn provider_bundle() -> Result<ProviderBundle, ProviderRegistrationError> {
    let writer = Arc::new(ExcelWriter::new());
    let mut bundle = ProviderBundle::default();
    bundle.add_source_provider(ExcelLoader)?;
    bundle.add_source_writer_arc(Arc::clone(&writer))?;
    bundle.add_table_manager_arc(writer)?;
    Ok(bundle)
}

#[derive(Debug, Default, Clone, Copy)]
pub struct ExcelLoader;

pub const EXCEL_LOADER_DESCRIPTOR: SourceProviderDescriptor = SourceProviderDescriptor {
    id: "excel",
    display_name: "Excel workbook",
    extensions: &["xlsx", "xlsm", "xls"],
    option_keys: &["sheets"],
};

impl SourceProvider for ExcelLoader {
    fn descriptor(&self) -> &'static SourceProviderDescriptor {
        &EXCEL_LOADER_DESCRIPTOR
    }

    fn probe(&self, source: &ProjectSourceRef<'_>) -> ProbeResult {
        if source.source_type == Some(EXCEL_LOADER_DESCRIPTOR.id) {
            return ProbeResult::certain();
        }
        if matches!(
            source.location,
            SourceLocationSpec::Path(path)
                if path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| EXCEL_LOADER_DESCRIPTOR.extensions.contains(&ext))
        ) {
            ProbeResult::likely()
        } else {
            ProbeResult::none()
        }
    }

    fn decode_options(&self, options: &Value) -> Result<DecodedSourceOptions, DiagnosticSet> {
        decode_excel_source_options(options)
    }

    fn resolve(
        &self,
        _ctx: SourceResolveContext<'_>,
        source: &ResolvedSource,
    ) -> Result<Vec<ResolvedSource>, DiagnosticSet> {
        let SourceLocationSpec::Path(path) = &source.location;
        if path.is_dir() {
            return collect_excel_sources(path, source);
        }
        if is_excel_path(path) {
            return Ok(vec![source.clone()]);
        }
        Err(DiagnosticSet::one(Diagnostic::error(
            "EXCEL-SOURCE",
            "EXCEL",
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
        let SourceLocationSpec::Path(file) = &source.location;
        let sheets = excel_sheets(excel_source_options(source)?);
        let excel_source = ExcelSource::new(file.clone(), sheets);
        collect_input_records(ctx.schema, &[excel_source])
            .map(|loaded| LoadedSource {
                records: loaded.records,
            })
            .map_err(excel_diagnostics_to_api)
    }
}

fn collect_excel_sources(
    dir: &Path,
    source: &ResolvedSource,
) -> Result<Vec<ResolvedSource>, DiagnosticSet> {
    let mut entries = fs::read_dir(dir)
        .map_err(|err| {
            DiagnosticSet::one(Diagnostic::error(
                "EXCEL-SOURCE",
                "EXCEL",
                format!("failed to read data source dir `{}`: {err}", dir.display()),
            ))
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| {
            DiagnosticSet::one(Diagnostic::error(
                "EXCEL-SOURCE",
                "EXCEL",
                format!("failed to read data source dir `{}`: {err}", dir.display()),
            ))
        })?;
    entries.sort_by_key(fs::DirEntry::path);

    let mut sources = Vec::new();
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            sources.extend(collect_excel_sources(&path, source)?);
        } else if is_excel_path(&path) {
            sources.push(ResolvedSource {
                provider_id: EXCEL_LOADER_DESCRIPTOR.id.to_string(),
                display_name: path.display().to_string(),
                location: SourceLocationSpec::Path(path),
                options: source.options.clone(),
            });
        }
    }
    Ok(sources)
}

fn is_excel_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| EXCEL_LOADER_DESCRIPTOR.extensions.contains(&ext))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::panic)]

    use super::*;
    use serde_json::json;
    #[test]
    fn rejects_empty_sheet_name_in_options() {
        let loader = ExcelLoader;
        let Err(err) = loader.decode_options(&json!({
            "sheets": [
                {
                    "sheet": "",
                    "columns": {
                        "A": "id"
                    }
                }
            ]
        })) else {
            panic!("empty sheet should fail");
        };

        assert!(err
            .iter()
            .any(|diagnostic| diagnostic.message == "excel source sheet `sheet` is empty"));
    }
}
