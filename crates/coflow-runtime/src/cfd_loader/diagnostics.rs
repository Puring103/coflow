use crate::api::{
    byte_range, map_diagnostics_with_origins, Diagnostic, DiagnosticSet, Label, LineIndex,
    SourceLocation,
};
use crate::data_model::{RecordOrigin, TextSpan};
use std::path::Path;

pub use coflow_core::loading::{
    CfdTextDiagnostic, CfdTextDiagnostics, CfdTextErrorCode, CfdTextLoadError, CfdTextSpan,
};

pub(super) fn text_span(line_index: &LineIndex, source: &str, span: CfdTextSpan) -> TextSpan {
    let range = line_index.range(source, span.start, span.end);
    TextSpan {
        start_line: range.start.line,
        start_character: range.start.character,
        end_line: range.end.line,
        end_character: range.end.character,
    }
}

pub(super) fn cfd_error_to_diagnostics(
    file: &Path,
    source: &str,
    err: CfdTextLoadError,
) -> DiagnosticSet {
    match err {
        CfdTextLoadError::Text(diagnostics) => DiagnosticSet {
            diagnostics: diagnostics
                .diagnostics
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
        },
        CfdTextLoadError::DataModel {
            diagnostics,
            origins,
        } => {
            let origins = origins
                .into_iter()
                .map(|origin| origin_with_file(file, origin))
                .collect::<Vec<_>>();
            map_diagnostics_with_origins(diagnostics, &origins)
        }
    }
}

fn origin_with_file(file: &Path, origin: RecordOrigin) -> RecordOrigin {
    match origin {
        RecordOrigin::File { path, span } if path.as_os_str().is_empty() => RecordOrigin::File {
            path: file.to_path_buf(),
            span,
        },
        other => other,
    }
}
