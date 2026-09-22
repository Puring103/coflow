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

use crate::api::{
    CfdLoadContext, CfdSource, Diagnostic, DiagnosticSet, LineIndex, LoadedCfdSource,
};

mod diagnostics;
use coflow_core::loading::lower_syntax;
mod writer;
use crate::data_model::RecordOrigin;
use diagnostics::{cfd_text_diagnostics, text_span};
use std::path::Path;
use std::sync::Arc;
pub(crate) use writer::{CfdWriter, CFD_WRITER_CAPABILITIES};

#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct CfdLoader;

impl CfdLoader {
    pub fn resolve(source: &CfdSource) -> Result<CfdSource, DiagnosticSet> {
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

    #[cfg(test)]
    pub fn load(
        ctx: CfdLoadContext<'_>,
        source: &CfdSource,
    ) -> Result<LoadedCfdSource, DiagnosticSet> {
        let loaded = Self::load_partial(ctx, source)?;
        if loaded.diagnostics.is_empty() {
            Ok(loaded)
        } else {
            Err(loaded.diagnostics)
        }
    }

    #[cfg(test)]
    pub(crate) fn load_partial(
        ctx: CfdLoadContext<'_>,
        source: &CfdSource,
    ) -> Result<LoadedCfdSource, DiagnosticSet> {
        Self::load_cached(ctx, source, &crate::CfdSourceStore::default())
    }

    pub(crate) fn load_cached(
        ctx: CfdLoadContext<'_>,
        source: &CfdSource,
        store: &crate::CfdSourceStore,
    ) -> Result<LoadedCfdSource, DiagnosticSet> {
        let file = source.location.path();
        let snapshot = match ctx.source_text {
            Some(text) => store.overlay(file, Arc::from(text)),
            None => store.read(file).map_err(|error| {
                DiagnosticSet::one(Diagnostic::error(
                    "CFD-READ",
                    "CFD",
                    format!("failed to read CFD source `{}`: {error}", file.display()),
                ))
            })?,
        };
        let contents = &snapshot.text;
        let (lowered, errors) = lower_syntax(ctx.schema, &snapshot.syntax, &snapshot.errors);
        // 行首索引只构建一次，避免逐条记录从文件头重新扫描（二次复杂度）。
        let line_index = LineIndex::new(&contents);
        let records = lowered
            .into_iter()
            .map(|record| {
                let span = text_span(&line_index, &contents, record.span);
                record.record.with_origin(RecordOrigin::File {
                    path: file.clone(),
                    span: Some(span),
                })
            })
            .collect();
        let diagnostics = cfd_text_diagnostics(file, &contents, errors);
        Ok(LoadedCfdSource {
            records,
            diagnostics,
            source: Arc::clone(&snapshot.text),
        })
    }
}

fn is_cfd_path(path: &Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()) == Some("cfd")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use std::fs;

    use std::path::PathBuf;

    use coflow_core::schema::{build_schema, parse_modules, CftFile, ModuleId};

    use super::CfdLoader;
    use crate::api::{CfdLoadContext, CfdSource, CfdSourcePath};
    use crate::{map_diagnostics_with_origins, origins_of, CfdDataModel, SourceLocation};

    fn schema() -> coflow_core::schema::CftSchema {
        let modules = parse_modules([CftFile::from_source(
            ModuleId::from("main"),
            "table Item { value: int; }",
        )]);
        build_schema(&modules).expect("schema")
    }

    #[test]
    fn rejects_non_cfd_sources_before_reading() {
        let source = CfdSource {
            location: CfdSourcePath::new("data/items.json"),
            display_name: "data/items.json".to_string(),
        };
        let diagnostics = CfdLoader::resolve(&source).expect_err("only CFD is supported");
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
        assert_record_text_spans(source_path, None);
    }

    #[test]
    fn source_overrides_preserve_record_text_spans() {
        // 内存来源走真实的项目加载入口，诊断仍保留来源路径和记录范围。
        assert_record_text_spans(
            PathBuf::from("memory.cfd"),
            Some("first: Item { value: 1 }\n\nsecond: Item {\n}\n"),
        );
    }

    fn assert_record_text_spans(source_path: PathBuf, source_text: Option<&str>) {
        let schema = schema();
        let loaded = CfdLoader::load(
            CfdLoadContext {
                schema: &schema,
                source_text,
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
