mod capabilities;
mod requests;
mod transaction;

pub use capabilities::{WriterCapabilities, WriterDescriptor};
pub use requests::{
    DeleteRecordRequest, InsertRecordRequest, RenameRecordRequest, RewriteRecordReferencesRequest,
    SpreadRewriteTarget, WriteBatchFailure, WriteCellRequest, WriteContext, WriteFieldPathSegment,
    WriteOutcome,
};
pub use transaction::{SourceTransaction, SourceTransactionCompensation};

use crate::{Diagnostic, DiagnosticSet, ResolvedSource};

/// Trait for source-specific writers that persist field edits.
///
/// Implementations dispatch on [`RecordOrigin`] to locate the cell/span and
/// write the new value to the source file. The runtime owns
/// transaction-level mutation reporting and generation rebuilds.
pub trait SourceWriter: Send + Sync {
    fn descriptor(&self) -> &'static WriterDescriptor;

    /// Return capabilities for one resolved source.
    ///
    /// Providers whose mutation support depends on the concrete storage
    /// format should override this instead of advertising provider-wide
    /// write access for every source they can read.
    fn capabilities(&self, _source: &ResolvedSource) -> WriterCapabilities {
        self.descriptor().capabilities.clone()
    }

    /// Start the rollback contract for one resolved source before any writer
    /// method mutates it.
    ///
    /// Local path sources use runtime-managed byte snapshots.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when the provider cannot initialize its transaction state.
    fn begin_transaction(
        &self,
        _ctx: WriteContext<'_>,
        source: &ResolvedSource,
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

    /// Persist multiple field changes for one resolved source. Providers may
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

    /// Rewrite source-level references to a renamed record key.
    ///
    /// The default implementation is a no-op because ordinary `CfdValue::Ref`
    /// locations are updated via [`SourceWriter::write_field`]. Providers should
    /// override this when their source syntax contains references that do not
    /// survive as runtime refs.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when the source cannot be read or updated.
    fn rewrite_record_references(
        &self,
        _ctx: WriteContext<'_>,
        _request: &RewriteRecordReferencesRequest<'_>,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        Ok(WriteOutcome::default())
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
}
