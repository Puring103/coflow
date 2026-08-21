mod capabilities;
mod requests;
mod transaction;

pub use capabilities::{CfdWriterDescriptor, WriterCapabilities};
pub use requests::{
    DeleteRecordRequest, InsertRecordRequest, RenameRecordRequest, ReorderRecordsOperation,
    ReorderRecordsRequest, WriteBatchFailure, WriteCellRequest, WriteContext,
    WriteFieldPathSegment, WriteOutcome, WriteRecordRef,
};
pub use transaction::{SourceTransaction, SourceTransactionCompensation};

use crate::{CfdSource, Diagnostic, DiagnosticSet};

/// Trait for source-specific writers that persist field edits.
///
/// Implementations dispatch on [`crate::data_model::RecordOrigin`] to locate the cell/span and
/// write the new value to the source file. The runtime owns
/// transaction-level mutation reporting and generation rebuilds.
pub trait CfdDocumentWriter: Send + Sync {
    fn descriptor(&self) -> &'static CfdWriterDescriptor;

    /// Return capabilities for one resolved source.
    ///
    /// The runtime has one concrete CFD storage format; this hook remains an
    /// internal seam for capability checks during mutation planning.
    fn capabilities(&self, _source: &CfdSource) -> WriterCapabilities {
        self.descriptor().capabilities.clone()
    }

    /// Start the rollback contract for one resolved source before any writer
    /// method mutates it.
    ///
    /// Local path sources use runtime-managed byte snapshots.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when the writer cannot initialize its transaction state.
    fn begin_transaction(
        &self,
        _ctx: WriteContext<'_>,
        source: &CfdSource,
    ) -> Result<SourceTransaction, DiagnosticSet> {
        let _ = source;
        Ok(SourceTransaction::RuntimeSnapshot)
    }

    /// Cheap pre-flight check: type matches, target file exists, etc. The
    /// default implementation does nothing.
    fn preflight(&self, _ctx: WriteContext<'_>, _request: &WriteCellRequest<'_>) -> DiagnosticSet {
        DiagnosticSet::empty()
    }

    /// Persist a single field change.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when the write cannot be performed (origin
    /// mismatch, missing file, transport error, schema-invalid value, etc.).
    fn write_field(
        &self,
        ctx: WriteContext<'_>,
        request: &WriteCellRequest<'_>,
    ) -> Result<WriteOutcome, DiagnosticSet>;

    /// Persist multiple field changes for one CFD source. The writer may
    /// override this to share source open/save work across the batch.
    ///
    /// # Errors
    ///
    /// Returns the failing request index and its diagnostics. The runtime
    /// compensates the enclosing source transaction on any failure.
    fn write_field_batch(
        &self,
        ctx: WriteContext<'_>,
        requests: &[WriteCellRequest<'_>],
    ) -> Result<Vec<WriteOutcome>, WriteBatchFailure> {
        requests
            .iter()
            .enumerate()
            .map(|(index, request)| {
                self.write_field(ctx, request)
                    .map_err(|diagnostics| WriteBatchFailure { index, diagnostics })
            })
            .collect()
    }

    /// Persist a new top-level record.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when the writer cannot insert records for this
    /// source or when the request cannot be represented by the source format.
    fn insert_record(
        &self,
        _ctx: WriteContext<'_>,
        _request: &InsertRecordRequest<'_>,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        Err(DiagnosticSet::one(Diagnostic::error(
            "WRITE-UNSUPPORTED",
            "WRITE",
            "writer does not support inserting records",
        )))
    }

    /// Rename a top-level record key.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when the writer cannot rename keys for this source
    /// or when the existing source no longer matches the requested old key.
    fn rename_record(
        &self,
        _ctx: WriteContext<'_>,
        _request: &RenameRecordRequest<'_>,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        Err(DiagnosticSet::one(Diagnostic::error(
            "WRITE-UNSUPPORTED",
            "WRITE",
            "writer does not support renaming record keys",
        )))
    }

    /// Delete a top-level record.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when the writer cannot delete records for this
    /// source or when the target no longer matches the requested record.
    fn delete_record(
        &self,
        _ctx: WriteContext<'_>,
        _request: &DeleteRecordRequest<'_>,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        Err(DiagnosticSet::one(Diagnostic::error(
            "WRITE-UNSUPPORTED",
            "WRITE",
            "writer does not support deleting records",
        )))
    }

    /// Atomically reorder top-level records inside one physical source container.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when the records do not share a container or the
    /// source no longer matches their recorded origins.
    fn reorder_records(
        &self,
        _ctx: WriteContext<'_>,
        _request: &ReorderRecordsRequest<'_>,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        Err(DiagnosticSet::one(Diagnostic::error(
            "WRITE-UNSUPPORTED",
            "WRITE",
            "writer does not support reordering records",
        )))
    }
}
