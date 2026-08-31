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
mod writer;
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
pub(crate) use writer::{CfdWriter, CFD_WRITER_CAPABILITIES};

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
pub(crate) struct CfdLoader;

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

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use std::fs;

    use coflow_language::{build_schema, parse_modules, CftDimensionInputs, CftFile, ModuleId};

    use super::CfdLoader;
    use crate::api::{CfdLoadContext, CfdSource, CfdSourcePath};
    use crate::{map_diagnostics_with_origins, origins_of, CfdDataModel, SourceLocation};

    fn schema() -> coflow_language::CftSchema {
        let modules = parse_modules([CftFile::from_source(
            ModuleId::from("main"),
            "type Item { value: int; }",
        )]);
        build_schema(&modules, &CftDimensionInputs::default()).expect("schema")
    }

    #[test]
    fn rejects_non_cfd_sources_before_reading() {
        let source = CfdSource {
            location: CfdSourcePath::new("data/items.json"),
            display_name: "data/items.json".to_string(),
        };
        let diagnostics = CfdLoader
            .resolve(&source)
            .expect_err("only CFD is supported");
        assert!(diagnostics.contains("unsupported extension"));
    }

    #[test]
    fn file_origins_preserve_record_text_spans() {
        let root = tempfile::tempdir().expect("temp source");
        let source_path = root.path().join("items.cfd");
        fs::write(
            &source_path,
            "first: Item { value: 1 }\n\nsecond: Item {\n}\n",
        )
        .expect("write source");
        let schema = schema();
        let loaded = CfdLoader
            .load(
                CfdLoadContext {
                    schema: &schema,
                    source_text: None,
                },
                &CfdSource {
                    location: CfdSourcePath::new(source_path.clone()),
                    display_name: source_path.display().to_string(),
                },
            )
            .expect("load source");
        let origins = origins_of(&loaded.records);
        let mut builder = CfdDataModel::builder(&schema);
        for record in loaded.records {
            builder.add_loaded_record(record);
        }
        let diagnostics = builder.build().expect_err("missing required value");
        let mapped = map_diagnostics_with_origins(diagnostics, &origins);
        assert!(matches!(
            mapped.diagnostics[0].primary.as_ref().map(|label| &label.location),
            Some(SourceLocation::FileSpan {
                path,
                start_line: 2,
                start_character: 0,
                end_line: 3,
                end_character: 1,
            }) if path == &source_path
        ));
    }
}

fn is_cfd_path(path: &Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()) == Some("cfd")
}
