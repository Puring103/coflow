//! Record views and write outcomes exposed at the engine boundary.

use coflow_api::{DiagnosticSet, RecordOrigin};
use coflow_data_model::{CfdRecord, CfdRecordId};
use serde::{Deserialize, Serialize};

use super::{RecordCoordinate, SourceId};

/// Read-only view of a top-level record. Bundles the model's `CfdRecord` with
/// the engine's metadata so hosts don't have to do a second lookup.
#[derive(Debug, Clone)]
pub struct RecordView<'a> {
    pub coordinate: RecordCoordinate,
    pub display_path: &'a str,
    pub record: &'a CfdRecord,
    pub origin: &'a RecordOrigin,
    pub source_id: SourceId,
    pub provider_id: &'a str,
}

/// Outcome of an engine write transaction. Surfaces both the rebuilt
/// diagnostics and the coordinates that changed so callers can refresh local
/// caches without re-querying the full project.
///
/// `renamed` is `Some(old, new)` when the write modified a record's `id`
/// field: the engine treats this as a coordinate change so the editor can
/// update routes, undo stacks, and any other long-lived references that
/// previously pointed at `old`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../frontend/src/bindings/"
    )
)]
pub struct WriteOutcome {
    pub touched: Vec<RecordCoordinate>,
    pub inserted: Option<RecordCoordinate>,
    pub deleted: Option<RecordCoordinate>,
    pub renamed: Option<(RecordCoordinate, RecordCoordinate)>,
    // Skip from TS: `DiagnosticSet` references concrete `Diagnostic` types
    // whose location data isn't part of the editor's surface. Hosts that
    // care convert to `FlatDiagnostic` before wire-shipping.
    #[cfg_attr(feature = "ts-export", ts(skip))]
    pub diagnostics: DiagnosticSet,
}

impl WriteOutcome {
    #[must_use]
    pub fn touch(coordinate: RecordCoordinate) -> Self {
        Self {
            touched: vec![coordinate],
            ..Default::default()
        }
    }
}

/// Unused: the editor still resolves writes via its own path. Kept here so
/// future hosts that want a uniform `(actual_type, key)` write surface can
/// import a single descriptor instead of stitching together coordinate +
/// record id at the call site.
#[derive(Debug, Clone)]
pub struct RecordTarget {
    pub id: CfdRecordId,
    pub coordinate: RecordCoordinate,
}
