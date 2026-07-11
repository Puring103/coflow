mod capabilities;
mod requests;
mod transaction;

pub use capabilities::{WriterCapabilities, WriterDescriptor};
pub use requests::{
    DeleteRecordRequest, InsertRecordRequest, RenameRecordRequest, RewriteRecordReferencesRequest,
    SpreadRewriteTarget, WriteCellRequest, WriteContext, WriteFieldPathSegment, WriteOutcome,
};
pub use transaction::{SourceTransaction, SourceTransactionCompensation};

use crate::{Diagnostic, DiagnosticSet, ResolvedSource, SourceLocationSpec};

/// Trait for source-specific writers that persist field edits.
///
/// Implementations dispatch on [`RecordOrigin`] to locate the cell/span, write
/// the new value to the source (file, remote API, ...), and report which
/// records were touched so the session can run incremental checks.
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
    /// Local path sources default to runtime-managed byte snapshots. Remote
    /// sources must explicitly return a provider compensation handle or are
    /// rejected by transactional mutation commands.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when the provider cannot initialize its remote
    /// transaction state.
    fn begin_transaction(
        &self,
        _ctx: WriteContext<'_>,
        source: &ResolvedSource,
    ) -> Result<SourceTransaction, DiagnosticSet> {
        Ok(match source.location {
            SourceLocationSpec::Path(_) => SourceTransaction::RuntimeSnapshot,
            SourceLocationSpec::Uri(_) => SourceTransaction::Unsupported,
        })
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
