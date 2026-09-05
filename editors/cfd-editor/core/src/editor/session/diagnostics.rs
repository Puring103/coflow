//! Editor-side view of the engine's diagnostics in wire-friendly
//! [`coflow_runtime::FlatDiagnostic`] shape.

use coflow_runtime::{path_to_slash, DiagnosticTarget, FlatDiagnostic, ProjectQueries};
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
            let (file_path, coordinate) = match &diagnostic.target {
                DiagnosticTarget::TableField {
                    file_path,
                    coordinate,
                    ..
                }
                | DiagnosticTarget::Record {
                    file_path,
                    coordinate,
                } => (file_path, coordinate),
                DiagnosticTarget::Source { .. }
                | DiagnosticTarget::ProjectSource { .. }
                | DiagnosticTarget::None => continue,
            };
            borrowed
                .entry(file_path)
                .or_default()
                .entry(coordinate.key.as_str())
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
    pub fn from_queries(queries: ProjectQueries<'_>, project_root: &Path) -> Self {
        diagnostics_from_store(queries, project_root)
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
                matches!(
                    &diagnostic.target,
                    DiagnosticTarget::TableField { coordinate: target, .. }
                        | DiagnosticTarget::Record { coordinate: target, .. }
                        if target.actual_type == coordinate.actual_type
                )
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
pub fn diagnostics_from_store(queries: ProjectQueries<'_>, project_root: &Path) -> Diagnostics {
    let store = queries.diagnostics();
    Diagnostics::from_items(
        store
            .as_set()
            .diagnostics
            .iter()
            .enumerate()
            .map(|(index, diagnostic)| {
                let logical = store.logical_location(index);
                let source_range = diagnostic.primary.as_ref().map(|label| label.location.text_range());
                let mut flat = diagnostic.flat_view(
                    logical.and_then(|loc| loc.actual_type.clone()),
                    logical.and_then(|loc| loc.record_key.clone()),
                    logical.and_then(|loc| loc.field_path.clone()),
                );
                normalize_target(&mut flat.target, source_range, queries, project_root);
                flat
            })
            .collect(),
    )
}

fn normalize_target(
    target: &mut DiagnosticTarget,
    source_range: Option<coflow_runtime::TextRange>,
    queries: ProjectQueries<'_>,
    project_root: &Path,
) {
    match target {
        DiagnosticTarget::TableField {
            file_path,
            coordinate,
            ..
        }
        | DiagnosticTarget::Record {
            file_path,
            coordinate,
        } => {
            *file_path = project_relative_path(project_root, file_path);
            if queries
                .file_for_record(&coordinate.actual_type, &coordinate.key)
                .is_none()
            {
                let file_path = file_path.clone();
                let range = source_range.or_else(|| queries
                    .rejected_records_by_coordinate(&coordinate.actual_type, &coordinate.key)
                    .find(|record| record.display_path == file_path)
                    .and_then(|record| match &record.origin {
                        coflow_runtime::RecordOrigin::File { span, .. } => *span,
                        coflow_runtime::RecordOrigin::None => None,
                    })
                    .map(|span| coflow_runtime::TextRange::from_parts(
                        span.start_line,
                        span.start_character,
                        span.end_line,
                        span.end_character,
                    )));
                *target = DiagnosticTarget::Source { file_path, range };
            }
        }
        DiagnosticTarget::Source { file_path, .. }
        | DiagnosticTarget::ProjectSource { file_path, .. } => {
            *file_path = project_relative_path(project_root, file_path);
        }
        DiagnosticTarget::None => {}
    }
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
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn record_index_preserves_order_and_matches_exact_coordinates() {
        let diagnostics = Diagnostics::from_items(vec![
            diagnostic("other.cfd", "sword", "Item", "unrelated file"),
            diagnostic("items.cfd", "shield", "Item", "unrelated record"),
            diagnostic("items.cfd", "sword", "Item", "first"),
            diagnostic("items.cfd", "sword", "Npc", "other type"),
            diagnostic("items.cfd", "sword", "Item", "second"),
        ]);

        let coordinate =
            RecordCoordinate::try_new("Item", "sword").expect("valid record coordinate");
        let messages = diagnostics
            .for_record("items.cfd", &coordinate)
            .map(|diagnostic| diagnostic.message.as_str())
            .collect::<Vec<_>>();

        assert_eq!(messages, vec!["first", "second"]);
    }

    fn diagnostic(
        file_path: &str,
        record_key: &str,
        actual_type: &str,
        message: &str,
    ) -> FlatDiagnostic {
        FlatDiagnostic {
            id: "test-diagnostic".to_string(),
            severity: "warning".to_string(),
            code: "TEST".to_string(),
            stage: "test".to_string(),
            message: message.to_string(),
            target: DiagnosticTarget::TableField {
                file_path: file_path.to_string(),
                coordinate: RecordCoordinate::try_new(
                    actual_type,
                    record_key,
                )
                .expect("coordinate"),
                field_path: "name".to_string(),
            },
            contexts: Vec::new(),
        }
    }
}
