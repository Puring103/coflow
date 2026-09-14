//! wire error types.
use coflow_runtime::FlatDiagnostic;
use serde::{Deserialize, Serialize};
#[cfg(feature = "ts-export")]
use ts_rs::TS;

/// Structured error returned by `SessionStore` methods.
///
/// Wire-shape: a discriminator (`kind`), a human-readable `message`, and an
/// optional list of structured `diagnostics` mirroring the same payload the
/// front-end already renders for build/load/check errors. The front-end can
/// route by `kind`, show `message` in a banner, and inject `diagnostics`
/// into the diagnostics panel without doing any string parsing.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
pub struct EditorError {
    pub kind: EditorErrorKind,
    pub message: String,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub diagnostics: Vec<FlatDiagnostic>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
#[serde(rename_all = "snake_case")]
pub enum EditorErrorKind {
    Session,
    Project,
    Write,
    NotFound,
    Other,
}

impl EditorError {
    #[must_use]
    pub fn new(kind: EditorErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            diagnostics: Vec::new(),
        }
    }

    #[must_use]
    pub fn session(message: impl Into<String>) -> Self {
        Self::new(EditorErrorKind::Session, message)
    }

    #[must_use]
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(EditorErrorKind::NotFound, message)
    }

    #[must_use]
    pub fn project(message: impl Into<String>) -> Self {
        Self::new(EditorErrorKind::Project, message)
    }

    #[must_use]
    pub fn write(message: impl Into<String>) -> Self {
        Self::new(EditorErrorKind::Write, message)
    }

    #[must_use]
    pub fn other(message: impl Into<String>) -> Self {
        Self::new(EditorErrorKind::Other, message)
    }

    #[must_use]
    pub fn with_diagnostics(mut self, diagnostics: Vec<FlatDiagnostic>) -> Self {
        self.diagnostics = diagnostics;
        self
    }
}

impl std::fmt::Display for EditorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for EditorError {}

impl From<String> for EditorError {
    fn from(message: String) -> Self {
        Self::other(message)
    }
}

impl From<&str> for EditorError {
    fn from(message: &str) -> Self {
        Self::other(message)
    }
}
