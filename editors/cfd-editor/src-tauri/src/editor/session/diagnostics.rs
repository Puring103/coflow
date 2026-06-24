//! Converts canonical diagnostics into the editor wire shape.

use crate::editor::types::DiagnosticItem;
use coflow_engine::DiagnosticsStore;
use std::path::Path;

use super::path::path_to_slash;

#[derive(Debug, Default, Clone)]
pub struct Diagnostics {
    items: Vec<DiagnosticItem>,
}

impl Diagnostics {
    #[must_use]
    pub const fn from_items(items: Vec<DiagnosticItem>) -> Self {
        Self { items }
    }

    #[must_use]
    pub fn flatten(&self) -> Vec<DiagnosticItem> {
        self.items.clone()
    }
}

#[must_use]
pub fn diagnostics_from_store(store: &DiagnosticsStore) -> Diagnostics {
    Diagnostics::from_items(
        store
            .as_set()
            .diagnostics
            .iter()
            .enumerate()
            .map(|(index, diagnostic)| {
                let mut item = diagnostic_from_api(diagnostic);
                if let Some(logical) = store.logical_location(index) {
                    item.record_key.clone_from(&logical.record_key);
                    item.field_path.clone_from(&logical.field_path);
                }
                item
            })
            .collect(),
    )
}

pub(super) fn diagnostic_from_api(d: &coflow_api::Diagnostic) -> DiagnosticItem {
    use coflow_api::{Severity, SourceLocation};
    let severity = match d.severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Info => "info",
    }
    .to_string();
    let file_path = d.primary.as_ref().map(|label| match &label.location {
        SourceLocation::FileSpan { path, .. }
        | SourceLocation::TableCell { path, .. }
        | SourceLocation::ProjectConfig { path, .. }
        | SourceLocation::Artifact { path } => path_to_slash(path),
        SourceLocation::RemoteCell { document, .. } => path_to_slash(Path::new(document)),
    });
    DiagnosticItem {
        severity,
        code: d.code.clone(),
        stage: d.stage.clone(),
        message: d.message.clone(),
        file_path,
        record_key: None,
        field_path: None,
    }
}
