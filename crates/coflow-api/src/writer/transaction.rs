/// Rollback guarantee declared by a writer for one resolved source.
pub enum SourceTransaction {
    /// Runtime snapshots the local source bytes and restores them on failure.
    RuntimeSnapshot,
}
impl std::fmt::Debug for SourceTransaction {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::RuntimeSnapshot => "RuntimeSnapshot",
        })
    }
}
