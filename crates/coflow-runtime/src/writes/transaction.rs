use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::api::{Diagnostic, DiagnosticSet};

use super::plan::MutationExecutionPlan;

#[derive(Debug, Default)]
pub(crate) struct MutationTransaction {
    local: LocalFileTransaction,
}

impl MutationTransaction {
    pub(crate) fn begin<'a>(
        plans: impl IntoIterator<Item = &'a MutationExecutionPlan>,
    ) -> Result<Self, DiagnosticSet> {
        let mut transaction = Self::default();
        let mut seen = std::collections::BTreeSet::new();
        for plan in plans {
            let enlisted = plan.visit_sources(|source| {
                if !seen.insert(source.location.path().clone()) {
                    return Ok(());
                }
                transaction.local.snapshot_file(source.location.path())
            });
            if let Err(mut diagnostics) = enlisted {
                transaction.local.rollback_into(&mut diagnostics);
                return Err(diagnostics);
            }
        }
        Ok(transaction)
    }

    pub(crate) fn compensate_into(self, diagnostics: &mut DiagnosticSet) {
        self.local.rollback_into(diagnostics);
    }
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
