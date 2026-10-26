//! 写入事实独立于诊断文本，供所有宿主判断重试和恢复边界。
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
#[serde(rename_all = "snake_case")]
pub enum PersistenceState {
    #[default]
    NotAttempted,
    Committed,
    Restored,
    Partial,
    Unknown,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
pub struct PublicationReport {
    pub state: PersistenceState,
    pub written_files: Vec<String>,
    pub restored_files: Vec<String>,
    pub restoration_failed: Vec<String>,
    pub retained_backups: std::collections::BTreeMap<String, String>,
}

impl PublicationReport {
    #[must_use]
    pub fn from_failure(failure: &coflow_staging::transaction::PublishFailure) -> Self {
        let paths =
            |items: &[std::path::PathBuf]| items.iter().map(|p| crate::path_to_slash(p)).collect();
        Self {
            state: if !failure.restoration_failed.is_empty() {
                PersistenceState::Partial
            } else if failure.written.is_empty() {
                PersistenceState::NotAttempted
            } else {
                PersistenceState::Restored
            },
            written_files: paths(&failure.written),
            restored_files: paths(&failure.restored),
            restoration_failed: paths(&failure.restoration_failed),
            retained_backups: failure
                .retained_backups
                .iter()
                .map(|(file, backup)| (crate::path_to_slash(file), crate::path_to_slash(backup)))
                .collect(),
        }
    }

    #[must_use]
    pub fn requires_recovery(&self) -> bool {
        matches!(
            self.state,
            PersistenceState::Partial | PersistenceState::Unknown
        )
    }
}

/// 发布源码时再次核对磁盘基线；与外部非协作写入者之间不承诺原子 CAS。
pub fn publish_source(
    path: &std::path::Path,
    expected: &str,
    source: &str,
) -> Result<(), crate::DiagnosticSet> {
    coflow_staging::transaction::publish(&[coflow_staging::transaction::FileChange {
        path: path.to_path_buf(),
        before: Some(expected.as_bytes().to_vec()),
        after: Some(source.as_bytes().to_vec()),
    }])
    .map(|_| ())
    .map_err(|failure| {
        let mut diagnostics = crate::DiagnosticSet::one(crate::Diagnostic::error(
            if failure.conflict {
                "WRITE-CONFLICT"
            } else {
                "SOURCE-WRITE"
            },
            "WRITE",
            failure.message.clone(),
        ));
        diagnostics.publication = Some(PublicationReport::from_failure(&failure));
        diagnostics
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_publication_preserves_an_external_change() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("source.cfd");
        std::fs::write(&path, "external").unwrap();
        let failure = publish_source(&path, "old", "local").unwrap_err();
        assert_eq!(
            failure.publication.unwrap().state,
            PersistenceState::NotAttempted
        );
        assert_eq!(std::fs::read_to_string(path).unwrap(), "external");
    }
}
