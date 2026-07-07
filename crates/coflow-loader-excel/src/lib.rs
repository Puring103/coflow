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
    DataLoader, Diagnostic, DiagnosticSet, LoadContext, LoadedRecords, LoaderDescriptor,
    ProbeResult, ProjectSourceRef, ResolvedSource, SourceLocationSpec, SourceResolveContext,
};
use std::fs;
use std::path::Path;

mod diagnostics;
mod options;
mod source;
pub mod writer;
use diagnostics::excel_diagnostics_to_api;
pub use diagnostics::{
    map_label_with_record_offset, ExcelDiagnostic, ExcelDiagnostics, ExcelLabel, ExcelLocation,
};
use options::excel_sheets_from_options;
pub use source::{collect_input_records, ExcelInputRecords, ExcelSheet, ExcelSource};
pub use writer::{ExcelWriter, EXCEL_WRITER_DESCRIPTOR};

#[derive(Debug, Default, Clone, Copy)]
pub struct ExcelLoader;

pub const EXCEL_LOADER_DESCRIPTOR: LoaderDescriptor = LoaderDescriptor {
    id: "excel",
    display_name: "Excel workbook",
    extensions: &["xlsx", "xlsm", "xls"],
    uri_schemes: &[],
    option_keys: &["sheets"],
};

impl DataLoader for ExcelLoader {
    fn descriptor(&self) -> &'static LoaderDescriptor {
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

    fn resolve(
        &self,
        _ctx: SourceResolveContext<'_>,
        source: &ResolvedSource,
    ) -> Result<Vec<ResolvedSource>, DiagnosticSet> {
        let SourceLocationSpec::Path(path) = &source.location else {
            if source.provider_id == EXCEL_LOADER_DESCRIPTOR.id {
                return Err(DiagnosticSet::one(Diagnostic::error(
                    "EXCEL-SOURCE",
                    "EXCEL",
                    "excel source requires `path`",
                )));
            }
            return Ok(Vec::new());
        };
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
        ctx: LoadContext<'_>,
        source: &ResolvedSource,
    ) -> Result<LoadedRecords, DiagnosticSet> {
        let SourceLocationSpec::Path(file) = &source.location else {
            return Err(DiagnosticSet::one(Diagnostic::error(
                "EXCEL-SOURCE",
                "EXCEL",
                "excel source requires `path`",
            )));
        };
        let sheets = excel_sheets_from_options(&source.options)?;
        let excel_source = ExcelSource::new(file.clone(), sheets);
        collect_input_records(ctx.schema, &[excel_source])
            .map(|loaded| LoadedRecords {
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
    use coflow_cft::CftContainer;
    use serde_json::json;
    use std::path::Path;

    #[test]
    fn rejects_empty_sheet_name_in_options() {
        let Err(err) = excel_sheets_from_options(&json!({
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

    #[test]
    fn explicit_excel_loader_rejects_url_source() {
        let loader = ExcelLoader;
        let schema = CftContainer::new();
        let source = ResolvedSource {
            provider_id: EXCEL_LOADER_DESCRIPTOR.id.to_string(),
            location: SourceLocationSpec::Uri("https://example.test/configs.xlsx".to_string()),
            options: json!({}),
            display_name: "https://example.test/configs.xlsx".to_string(),
        };

        let Err(err) = loader.resolve(
            SourceResolveContext {
                project_root: Path::new("."),
                schema: &schema,
            },
            &source,
        ) else {
            panic!("excel url source should fail");
        };

        assert!(err
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("excel source requires `path`")));
    }
}
