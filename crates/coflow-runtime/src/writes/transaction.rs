use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use coflow_api::{
    Diagnostic, DiagnosticSet, ResolvedSource, SourceLocationSpec, SourceTransaction,
    SourceTransactionCompensation, SourceWriter, WriteContext,
};

#[derive(Debug, Default)]
pub(crate) struct MutationTransaction {
    local: LocalFileTransaction,
    providers: Vec<ProviderTransaction>,
}

impl MutationTransaction {
    pub(crate) fn begin(
        ctx: WriteContext<'_>,
        sources: impl IntoIterator<Item = (ResolvedSource, Arc<dyn SourceWriter>)>,
    ) -> Result<Self, DiagnosticSet> {
        let mut transaction = Self::default();
        let mut seen = std::collections::BTreeSet::new();
        for (source, writer) in sources {
            let key = source_key(&source);
            if !seen.insert(key) {
                continue;
            }
            let declared = match writer.begin_transaction(ctx, &source) {
                Ok(declared) => declared,
                Err(mut diagnostics) => {
                    transaction.abort_into(&mut diagnostics);
                    return Err(diagnostics);
                }
            };
            match declared {
                SourceTransaction::RuntimeSnapshot => {
                    let SourceLocationSpec::Path(path) = &source.location else {
                        let mut diagnostics = DiagnosticSet::one(Diagnostic::error(
                            "WRITE-TXN-CONTRACT",
                            "WRITE",
                            format!(
                                "provider `{}` requested a runtime snapshot for non-local source `{}`",
                                source.provider_id, source.display_name
                            ),
                        ));
                        transaction.abort_into(&mut diagnostics);
                        return Err(diagnostics);
                    };
                    if let Err(mut diagnostics) = transaction.local.snapshot_file(path) {
                        transaction.abort_into(&mut diagnostics);
                        return Err(diagnostics);
                    }
                }
                SourceTransaction::Compensation(compensation) => {
                    transaction.providers.push(ProviderTransaction {
                        source: source.display_name.clone(),
                        compensation,
                    });
                }
                SourceTransaction::Unsupported => {
                    let mut diagnostics = SourceTransaction::unsupported_diagnostic(&source);
                    transaction.abort_into(&mut diagnostics);
                    return Err(diagnostics);
                }
            }
        }
        Ok(transaction)
    }

    pub(crate) fn commit(mut self) -> Result<(), DiagnosticSet> {
        let mut failure = None;
        for provider in &mut self.providers {
            if let Err(provider_diagnostics) = provider.compensation.commit() {
                failure = Some((provider.source.clone(), provider_diagnostics));
                break;
            }
        }
        let Some((source, provider_diagnostics)) = failure else {
            return Ok(());
        };
        let mut diagnostics = DiagnosticSet::one(transaction_error(
            "WRITE-TXN-COMMIT",
            &source,
            "commit",
        ));
        diagnostics.extend(provider_diagnostics);
        self.compensate_into(&mut diagnostics);
        Err(diagnostics)
    }

    pub(crate) fn compensate_into(mut self, diagnostics: &mut DiagnosticSet) {
        for provider in self.providers.iter_mut().rev() {
            if let Err(provider_diagnostics) = provider.compensation.compensate() {
                diagnostics.push(transaction_error(
                    "WRITE-TXN-COMPENSATE",
                    &provider.source,
                    "compensate",
                ));
                diagnostics.extend(provider_diagnostics);
            }
        }
        self.local.rollback_into(diagnostics);
    }

    fn abort_into(&mut self, diagnostics: &mut DiagnosticSet) {
        for provider in self.providers.iter_mut().rev() {
            if let Err(provider_diagnostics) = provider.compensation.abort() {
                diagnostics.push(transaction_error(
                    "WRITE-TXN-ABORT",
                    &provider.source,
                    "abort",
                ));
                diagnostics.extend(provider_diagnostics);
            }
        }
    }
}

struct ProviderTransaction {
    source: String,
    compensation: Box<dyn SourceTransactionCompensation>,
}

impl std::fmt::Debug for ProviderTransaction {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProviderTransaction")
            .field("source", &self.source)
            .field("compensation", &"..")
            .finish()
    }
}

fn source_key(source: &ResolvedSource) -> String {
    match &source.location {
        SourceLocationSpec::Path(path) => {
            format!("{}:path:{}", source.provider_id, path.display())
        }
        SourceLocationSpec::Uri(uri) => format!("{}:uri:{uri}", source.provider_id),
    }
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
        match &self.original {
            Some(bytes) => fs::write(&self.path, bytes),
            None => match fs::remove_file(&self.path) {
                Ok(()) => Ok(()),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(err) => Err(err),
            },
        }
    }
}
