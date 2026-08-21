use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::api::{
    CfdSource, Diagnostic, DiagnosticSet, SourceTransaction, SourceTransactionCompensation,
    WriteContext,
};

use super::plan::MutationExecutionPlan;

#[derive(Debug, Default)]
pub(crate) struct MutationTransaction {
    local: LocalFileTransaction,
    compensations: Vec<CompensationTransaction>,
}

impl MutationTransaction {
    pub(crate) fn begin<'a>(
        ctx: WriteContext<'_>,
        plans: impl IntoIterator<Item = &'a MutationExecutionPlan>,
    ) -> Result<Self, DiagnosticSet> {
        let mut transaction = Self::default();
        let mut seen = std::collections::BTreeSet::new();
        for plan in plans {
            let enlisted = plan.visit_sources(|source, writer| {
                let key = source_key(source);
                if !seen.insert(key) {
                    return Ok(());
                }
                let declared = writer.map_or_else(
                    || Ok(SourceTransaction::RuntimeSnapshot),
                    |writer| writer.begin_transaction(ctx, source),
                )?;
                transaction.enlist(source, declared)
            });
            if let Err(mut diagnostics) = enlisted {
                transaction.abort_into(&mut diagnostics);
                return Err(diagnostics);
            }
        }
        Ok(transaction)
    }

    fn enlist(
        &mut self,
        source: &CfdSource,
        declared: SourceTransaction,
    ) -> Result<(), DiagnosticSet> {
        match declared {
            SourceTransaction::RuntimeSnapshot => {
                let path = source.location.path();
                self.local.snapshot_file(path)?;
            }
            SourceTransaction::Compensation(compensation) => {
                self.compensations.push(CompensationTransaction {
                    source: source.display_name.clone(),
                    compensation,
                });
            }
            SourceTransaction::Unsupported => {
                return Err(SourceTransaction::unsupported_diagnostic(source));
            }
        }
        Ok(())
    }

    pub(crate) fn commit(mut self) -> Result<(), DiagnosticSet> {
        let mut failure = None;
        for writer in &mut self.compensations {
            if let Err(writer_diagnostics) = writer.compensation.prepare_commit() {
                failure = Some((writer.source.clone(), writer_diagnostics));
                break;
            }
        }
        if let Some((source, writer_diagnostics)) = failure {
            let mut diagnostics = DiagnosticSet::one(transaction_error(
                "WRITE-TXN-COMMIT",
                &source,
                "prepare publication for",
            ));
            diagnostics.extend(writer_diagnostics);
            self.compensate_into(&mut diagnostics);
            return Err(diagnostics);
        }
        for writer in &mut self.compensations {
            writer.compensation.commit();
        }
        Ok(())
    }

    pub(crate) fn compensate_into(mut self, diagnostics: &mut DiagnosticSet) {
        for writer in self.compensations.iter_mut().rev() {
            if let Err(writer_diagnostics) = writer.compensation.compensate() {
                diagnostics.push(transaction_error(
                    "WRITE-TXN-COMPENSATE",
                    &writer.source,
                    "compensate",
                ));
                diagnostics.extend(writer_diagnostics);
            }
        }
        self.local.rollback_into(diagnostics);
    }

    fn abort_into(&mut self, diagnostics: &mut DiagnosticSet) {
        for writer in self.compensations.iter_mut().rev() {
            if let Err(writer_diagnostics) = writer.compensation.abort() {
                diagnostics.push(transaction_error(
                    "WRITE-TXN-ABORT",
                    &writer.source,
                    "abort",
                ));
                diagnostics.extend(writer_diagnostics);
            }
        }
    }
}

struct CompensationTransaction {
    source: String,
    compensation: Box<dyn SourceTransactionCompensation>,
}

impl std::fmt::Debug for CompensationTransaction {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CompensationTransaction")
            .field("source", &self.source)
            .field("compensation", &"..")
            .finish()
    }
}

fn source_key(source: &CfdSource) -> String {
    let path = source.location.path();
    format!("path:{}", path.display())
}

fn transaction_error(code: &str, source: &str, operation: &str) -> Diagnostic {
    Diagnostic::error(
        code,
        "WRITE",
        format!("failed to {operation} source transaction for `{source}`"),
    )
}

#[derive(Debug, Default)]
pub(super) struct LocalFileTransaction {
    snapshots: BTreeMap<PathBuf, FileSnapshot>,
}

impl LocalFileTransaction {
    pub(super) fn rollback_into(self, diagnostics: &mut DiagnosticSet) {
        for snapshot in self.snapshots.into_values().rev() {
            if let Err(err) = snapshot.restore() {
                diagnostics.push(Diagnostic::error(
                    "WRITE-ROLLBACK",
                    "WRITE",
                    format!(
                        "failed to roll back source `{}` after write failure: {err}",
                        snapshot.path.display()
                    ),
                ));
            }
        }
    }

    fn snapshot_file(&mut self, path: &Path) -> Result<(), DiagnosticSet> {
        if self.snapshots.contains_key(path) {
            return Ok(());
        }
        let original = match fs::read(path) {
            Ok(bytes) => Some(bytes),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
            Err(err) => {
                return Err(DiagnosticSet::one(Diagnostic::error(
                    "WRITE-TXN",
                    "WRITE",
                    format!(
                        "failed to snapshot source `{}` before write: {err}",
                        path.display()
                    ),
                )));
            }
        };
        self.snapshots.insert(
            path.to_path_buf(),
            FileSnapshot {
                path: path.to_path_buf(),
                original,
            },
        );
        Ok(())
    }
}

#[derive(Debug)]
struct FileSnapshot {
    path: PathBuf,
    original: Option<Vec<u8>>,
}

impl FileSnapshot {
    fn restore(&self) -> std::io::Result<()> {
        self.original.as_ref().map_or_else(
            || match fs::remove_file(&self.path) {
                Ok(()) => Ok(()),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(err) => Err(err),
            },
            |bytes| fs::write(&self.path, bytes),
        )
    }
}
