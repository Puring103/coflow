use crate::data_model::{CfdDataModel, CfdPathSegment, CfdValue, RecordOrigin};
use crate::{CfdSource, DiagnosticSet};
use coflow_language::CftSchema;
use std::collections::BTreeMap;
use std::path::Path;

/// Canonical data-model path segment used by writers and host wire adapters.
pub type WriteFieldPathSegment = CfdPathSegment;

/// Request describing a single field write.
#[derive(Debug, Clone)]
pub struct WriteCellRequest<'a> {
    pub origin: &'a RecordOrigin,
    pub record_key: &'a str,
    pub actual_type: &'a str,
    pub field_path: &'a [WriteFieldPathSegment],
    /// Source-neutral new value, serialized to the source format by the writer.
    pub new_value: &'a CfdValue,
    /// Optional pre-resolved schema type for the record. Writers that produce
    /// typed source representations (e.g. CFD) use this for serialization.
    pub schema: &'a CftSchema,
    /// Original CFD source that produced the record.
    pub source: &'a CfdSource,
}

/// Request describing a new top-level record insertion.
#[derive(Debug, Clone)]
pub struct InsertRecordRequest<'a> {
    /// Target source that should receive the new record.
    pub source: &'a CfdSource,
    /// CFD records are document-addressed; no sheet/table selector is needed.
    pub record_key: &'a str,
    pub actual_type: &'a str,
    pub fields: &'a BTreeMap<String, CfdValue>,
    pub schema: &'a CftSchema,
    /// Insert immediately before this existing record, or append when absent.
    pub before: Option<WriteRecordRef<'a>>,
}

/// Request describing a top-level record deletion.
#[derive(Debug, Clone)]
pub struct DeleteRecordRequest<'a> {
    pub origin: &'a RecordOrigin,
    pub record_key: &'a str,
    pub actual_type: &'a str,
    pub source: &'a CfdSource,
}

/// A stable record identity paired with its CFD physical origin.
#[derive(Debug, Clone, Copy)]
pub struct WriteRecordRef<'a> {
    pub origin: &'a RecordOrigin,
    pub record_key: &'a str,
    pub actual_type: &'a str,
}

/// One atomic record-order change inside a physical source container.
#[derive(Debug, Clone, Copy)]
pub enum ReorderRecordsOperation<'a> {
    Swap {
        first: WriteRecordRef<'a>,
        second: WriteRecordRef<'a>,
    },
    /// Move `record` immediately before `before`, or to the end when the
    /// anchor is absent.
    MoveBefore {
        record: WriteRecordRef<'a>,
        before: Option<WriteRecordRef<'a>>,
    },
}

/// Request describing one atomic top-level record reorder.
#[derive(Debug, Clone, Copy)]
pub struct ReorderRecordsRequest<'a> {
    pub source: &'a CfdSource,
    pub operation: ReorderRecordsOperation<'a>,
}

/// Request describing a top-level record key rename.
#[derive(Debug, Clone)]
pub struct RenameRecordRequest<'a> {
    pub origin: &'a RecordOrigin,
    pub old_key: &'a str,
    pub new_key: &'a str,
    pub actual_type: &'a str,
    pub source: &'a CfdSource,
    pub schema: &'a CftSchema,
}

/// Writer diagnostics produced by a successful CFD writer call.
///
/// The runtime owns mutation lifecycle reporting and rebuilds the published
/// generation after the complete transaction. The writer therefore reports only
/// source-specific diagnostics here, not a second copy of mutation metadata.
#[derive(Debug, Clone, Default)]
pub struct WriteOutcome {
    /// Optional non-fatal diagnostics surfaced to the user.
    pub diagnostics: DiagnosticSet,
}

#[derive(Debug, Clone)]
pub struct WriteBatchFailure {
    pub index: usize,
    pub diagnostics: DiagnosticSet,
}

/// Context passed to writers. Mirrors [`crate::CfdLoadContext`] but for writes.
#[derive(Debug, Clone, Copy)]
pub struct WriteContext<'a> {
    pub project_root: &'a Path,
    pub schema: &'a CftSchema,
    /// The current data model. Writers use it to resolve
    /// [`crate::data_model::CfdRecordId`]s
    /// inside the request value (e.g. for ref serialization). May be `None`
    /// when running pre-flight on a value that hasn't been merged into the
    /// model yet.
    pub model: Option<&'a CfdDataModel>,
}
