use crate::{CfdSource, DiagnosticSet};

/// Writer-owned compensation state for a source transaction.
///
/// `abort` releases state when enlistment fails before writes start.
/// `compensate` restores staged writes after later failures. Publication is
/// two phase so every compensation remains available until all prepares finish.
pub trait SourceTransactionCompensation: Send {
    /// Release transaction state before source mutation starts.
    ///
    /// # Errors
    ///
    /// Returns writer diagnostics when transaction state cannot be released.
    fn abort(&mut self) -> Result<(), DiagnosticSet>;

    /// Restore the source after a staged mutation.
    ///
    /// # Errors
    ///
    /// Returns writer diagnostics when the source cannot be restored completely.
    fn compensate(&mut self) -> Result<(), DiagnosticSet>;

    /// Publish staged writes while retaining enough state to compensate them.
    ///
    /// The runtime calls this for every compensation before finalizing writes.
    /// A successful implementation must remain compensatable until `commit`.
    ///
    /// # Errors
    ///
    /// Returns writer diagnostics when staged writes cannot be published.
    fn prepare_commit(&mut self) -> Result<(), DiagnosticSet>;

    /// Release compensation state after every write published successfully.
    fn commit(&mut self);
}

/// Rollback guarantee declared by a writer for one resolved source.
pub enum SourceTransaction {
    /// Runtime snapshots the local source bytes and restores them on failure.
    RuntimeSnapshot,
    /// The writer owns transaction state and can compensate writes.
    Compensation(Box<dyn SourceTransactionCompensation>),
    /// The source cannot participate in an atomic mutation transaction.
    Unsupported,
}
impl std::fmt::Debug for SourceTransaction {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::RuntimeSnapshot => "RuntimeSnapshot",
            Self::Compensation(_) => "Compensation(..)",
            Self::Unsupported => "Unsupported",
        })
    }
}

impl SourceTransaction {
    #[must_use]
    pub fn unsupported_diagnostic(source: &CfdSource) -> DiagnosticSet {
        DiagnosticSet::one(crate::Diagnostic::error(
            "WRITE-TXN-UNSUPPORTED",
            "WRITE",
            format!(
                "CFD writer does not declare compensation for source `{}`",
                source.display_name
            ),
        ))
    }
}
