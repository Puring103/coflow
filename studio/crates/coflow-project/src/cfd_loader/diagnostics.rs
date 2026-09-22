use crate::api::{byte_range, Diagnostic, DiagnosticSet, Label, LineIndex, SourceLocation};
use crate::data_model::TextSpan;
use std::path::Path;

use coflow_core::loading::{CfdTextDiagnostic, CfdTextSpan};

pub(super) fn text_span(line_index: &LineIndex, source: &str, span: CfdTextSpan) -> TextSpan {
    let range = line_index.range(source, span.start, span.end);
    TextSpan {
        start_line: range.start.line,
        start_character: range.start.character,
        end_line: range.end.line,
        end_character: range.end.character,
    }
}

pub(super) fn cfd_text_diagnostics(
    file: &Path,
    source: &str,
    diagnostics: Vec<CfdTextDiagnostic>,
) -> DiagnosticSet {
    DiagnosticSet {
        diagnostics: diagnostics
            .into_iter()
            .map(|diagnostic| {
                let range = byte_range(source, diagnostic.span.start, diagnostic.span.end);
                Diagnostic::error(
                    format!("CFD-TEXT-{:?}", diagnostic.code),
                    "CFD",
                    diagnostic.message,
                )
                .with_primary(Label {
                    location: SourceLocation::FileSpan {
                        path: file.to_path_buf(),
                        start_line: range.start.line,
                        start_character: range.start.character,
                        end_line: range.end.line,
                        end_character: range.end.character,
                    },
                    message: None,
                })
            })
            .collect(),
    }
}
