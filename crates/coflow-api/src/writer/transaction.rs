use crate::{DiagnosticSet, ResolvedSource};

/// Provider-owned compensation state for a remote source transaction.
///
/// `abort` is called when planning fails before source mutation starts.
/// `compensate` is called after at least one staged writer operation or a
/// post-write rebuild failure. `commit` publishes the provider transaction.
pub trait SourceTransactionCompensation: Send {
    /// Release transaction state when no mutation was applied.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when remote transaction state cannot be released.
    fn abort(&mut self) -> Result<(), DiagnosticSet>;

    /// Restore the source after staged mutation failed.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when the provider cannot restore its snapshot.
    fn compensate(&mut self) -> Result<(), DiagnosticSet>;

    /// Publish a successful source transaction.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when the provider cannot commit its transaction.
    fn commit(&mut self) -> Result<(), DiagnosticSet>;
}

/// Rollback guarantee declared by a writer for one resolved source.
pub enum SourceTransaction {
    /// Runtime snapshots the local source bytes and restores them on failure.
    RuntimeSnapshot,
    /// Provider owns a remote snapshot/transaction and can compensate writes.
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
    pub fn unsupported_diagnostic(source: &ResolvedSource) -> DiagnosticSet {
        DiagnosticSet::one(crate::Diagnostic::error(
            "WRITE-TXN-UNSUPPORTED",
            "WRITE",
            format!(
                "provider `{}` does not declare compensation for source `{}`",
                source.provider_id,
                source.display_name
            ),
        ))
    }
}
