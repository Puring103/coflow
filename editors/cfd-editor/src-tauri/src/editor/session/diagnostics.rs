//! Editor-side view of the engine's diagnostics in wire-friendly
//! [`coflow_api::FlatDiagnostic`] shape.

use coflow_api::{path_to_slash, FlatDiagnostic};
use coflow_runtime::DiagnosticsStore;
use std::collections::HashMap;
use std::path::Path;

use coflow_runtime::RecordCoordinate;

#[derive(Debug, Default, Clone)]
pub struct Diagnostics {
    items: Vec<FlatDiagnostic>,
    by_file_record: HashMap<String, HashMap<String, Vec<usize>>>,
}

impl Diagnostics {
    #[must_use]
    pub fn from_items(items: Vec<FlatDiagnostic>) -> Self {
        let mut borrowed = HashMap::<&str, HashMap<&str, Vec<usize>>>::new();
        for (index, diagnostic) in items.iter().enumerate() {
            let (Some(file_path), Some(record_key)) =
                (&diagnostic.file_path, &diagnostic.record_key)
            else {
                continue;
            };
            borrowed
                .entry(file_path)
                .or_default()
                .entry(record_key)
                .or_default()
                .push(index);
        }
        let by_file_record = borrowed
            .into_iter()
            .map(|(file_path, by_record)| {
                let by_record = by_record
                    .into_iter()
                    .map(|(record_key, indexes)| (record_key.to_string(), indexes))
                    .collect();
                (file_path.to_string(), by_record)
            })
            .collect();
        Self {
            items,
            by_file_record,
        }
    }

    #[must_use]
    pub fn from_store(store: &DiagnosticsStore, project_root: &Path) -> Self {
        diagnostics_from_store(store, project_root)
    }

    #[must_use]
    pub fn to_wire(&self) -> Vec<FlatDiagnostic> {
        self.items.clone()
    }

    pub fn for_record<'a>(
        &'a self,
        file_path: &str,
        coordinate: &'a RecordCoordinate,
    ) -> impl Iterator<Item = &'a FlatDiagnostic> + 'a {
        self.by_file_record
            .get(file_path)
            .and_then(|by_record| by_record.get(coordinate.key.as_str()))
            .into_iter()
            .flatten()
            .filter_map(|index| self.items.get(*index))
            .filter(move |diagnostic| {
                diagnostic
                    .actual_type
                    .as_deref()
                    .is_none_or(|actual_type| actual_type == coordinate.actual_type)
            })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_index_preserves_order_and_matches_untyped_diagnostics() {
        let diagnostics = Diagnostics::from_items(vec![
            diagnostic("other.cfd", "sword", Some("Item"), "unrelated file"),
            diagnostic("items.cfd", "shield", Some("Item"), "unrelated record"),
            diagnostic("items.cfd", "sword", None, "untyped"),
            diagnostic("items.cfd", "sword", Some("Npc"), "other type"),
            diagnostic("items.cfd", "sword", Some("Item"), "typed"),
        ]);

        let coordinate = RecordCoordinate::new("Item", "sword");
        let messages = diagnostics
            .for_record("items.cfd", &coordinate)
            .map(|diagnostic| diagnostic.message.as_str())
            .collect::<Vec<_>>();

        assert_eq!(messages, vec!["untyped", "typed"]);
    }

    fn diagnostic(
        file_path: &str,
        record_key: &str,
        actual_type: Option<&str>,
        message: &str,
    ) -> FlatDiagnostic {
        FlatDiagnostic {
            severity: "warning".to_string(),
            code: "TEST".to_string(),
            stage: "test".to_string(),
            message: message.to_string(),
            file_path: Some(file_path.to_string()),
            actual_type: actual_type.map(str::to_string),
            record_key: Some(record_key.to_string()),
            field_path: Some("name".to_string()),
        }
    }
}
