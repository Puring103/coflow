//! Editor-side view of the engine's diagnostics in wire-friendly
//! [`coflow_api::FlatDiagnostic`] shape.

use coflow_api::FlatDiagnostic;
use coflow_engine::DiagnosticsStore;

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
    pub fn from_store(store: &DiagnosticsStore) -> Self {
        diagnostics_from_store(store)
    }

    #[must_use]
    pub fn flatten(&self) -> Vec<FlatDiagnostic> {
        self.items.clone()
    }
}

/// Convert engine diagnostics + logical locations to wire shape. Used for
/// the initial snapshot returned by `load_project`.
#[must_use]
pub fn diagnostics_from_store(store: &DiagnosticsStore) -> Diagnostics {
    Diagnostics::from_items(
        store
            .as_set()
            .diagnostics
            .iter()
            .enumerate()
            .map(|(index, diagnostic)| {
                let logical = store.logical_location(index);
                diagnostic.flat_view(
                    logical.and_then(|loc| loc.actual_type.clone()),
                    logical.and_then(|loc| loc.record_key.clone()),
                    logical.and_then(|loc| loc.field_path.clone()),
                )
            })
            .collect(),
    )
}
