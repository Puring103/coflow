//! Editor-side view of the engine's diagnostics in wire-friendly
//! [`coflow_api::FlatDiagnostic`] shape.

use coflow_api::{Diagnostic, DiagnosticSet, FlatDiagnostic};
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

    /// Construct from a freshly rebuilt `DiagnosticSet`. The engine's
    /// `write_field` returns the post-write set; we flatten it into wire
    /// shape so the front-end's diagnostics panel sees the right state
    /// without an extra query.
    ///
    /// Logical (record / field) locations are not reconstructed here — the
    /// session does that lazily via [`diagnostics_from_store`] on the next
    /// full snapshot.
    #[must_use]
    pub fn from_set(set: DiagnosticSet) -> Self {
        Self::from_items(set.diagnostics.iter().map(flat_view_no_logical).collect())
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
                    logical.and_then(|loc| loc.record_key.clone()),
                    logical.and_then(|loc| loc.field_path.clone()),
                )
            })
            .collect(),
    )
}

fn flat_view_no_logical(d: &Diagnostic) -> FlatDiagnostic {
    d.flat_view(None, None)
}
