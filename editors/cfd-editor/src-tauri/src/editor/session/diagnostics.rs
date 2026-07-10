//! Editor-side view of the engine's diagnostics in wire-friendly
//! [`coflow_api::FlatDiagnostic`] shape.

use coflow_api::{path_to_slash, FlatDiagnostic};
use coflow_runtime::DiagnosticsStore;
use std::path::Path;

#[derive(Debug, Default, Clone)]
pub struct Diagnostics {
    items: Vec<FlatDiagnostic>,
}

impl Diagnostics {
    #[must_use]
    pub const fn from_items(items: Vec<FlatDiagnostic>) -> Self {
        Self { items }
    }

    #[must_use]
    pub fn from_store(store: &DiagnosticsStore, project_root: &Path) -> Self {
        diagnostics_from_store(store, project_root)
    }

    #[must_use]
    pub fn flatten(&self) -> Vec<FlatDiagnostic> {
        self.items.clone()
    }
}

/// Convert engine diagnostics + logical locations to wire shape. Used for
/// the initial snapshot returned by `load_project`.
///
/// The engine records absolute file paths in `SourceLocation`, but the
/// editor front-end works in project-relative paths (matching what appears
/// in `FileTreeNode` and `FileRecords`). We normalize `file_path` here so the
/// diagnostics-panel jump buttons and per-record/field angle badges can
/// match against the same key the rest of the UI uses.
#[must_use]
pub fn diagnostics_from_store(store: &DiagnosticsStore, project_root: &Path) -> Diagnostics {
    Diagnostics::from_items(
        store
            .as_set()
            .diagnostics
            .iter()
            .enumerate()
            .map(|(index, diagnostic)| {
                let logical = store.logical_location(index);
                let mut flat = diagnostic.flat_view(
                    logical.and_then(|loc| loc.actual_type.clone()),
                    logical.and_then(|loc| loc.record_key.clone()),
                    logical.and_then(|loc| loc.field_path.clone()),
                );
                if let Some(path) = flat.file_path.take() {
                    flat.file_path = Some(project_relative_path(project_root, &path));
                }
                flat
            })
            .collect(),
    )
}

/// Best-effort conversion of an engine-emitted absolute file path back to
/// the project-relative form used elsewhere in the wire protocol. If the
/// path is already relative or doesn't sit under `project_root`, it's
/// returned unchanged so we never silently strip an unrelated prefix.
fn project_relative_path(project_root: &Path, path: &str) -> String {
    let candidate = Path::new(path);
    let root = normalize(project_root);
    let normalized = normalize(candidate);
    if let Some(rest) = normalized.strip_prefix(&root) {
        let trimmed = rest.trim_start_matches('/');
        return trimmed.to_string();
    }
    path.to_string()
}

fn normalize(path: &Path) -> String {
    path_to_slash(path)
}
