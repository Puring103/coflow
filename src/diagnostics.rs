use coflow_runtime::{
    Diagnostic, DiagnosticSet, Label, SourceLocation,
};
use std::path::Path;

#[must_use]
pub fn cli_error(code: impl Into<String>, message: impl Into<String>) -> DiagnosticSet {
    DiagnosticSet::one(Diagnostic::error(code, "CLI", message))
}

#[must_use]
pub fn cli_file_error(
    path: &Path,
    code: impl Into<String>,
    message: impl Into<String>,
) -> DiagnosticSet {
    DiagnosticSet::one(Diagnostic::error(code, "CLI", message).with_primary(Label {
        location: SourceLocation::Artifact {
            path: path.to_path_buf(),
        },
        message: None,
    }))
}

