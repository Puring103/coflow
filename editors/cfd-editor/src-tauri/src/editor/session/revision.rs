use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct RevisionTicket(u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ContentRevision([u8; 32]);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ExpectedFileRevision {
    Present(ContentRevision),
    Missing,
}

#[derive(Debug)]
pub(super) struct RevisionCoordinator {
    current: u32,
    expected_files: HashMap<PathBuf, ExpectedFileRevision>,
}

impl RevisionCoordinator {
    pub(super) fn initial() -> Self {
        Self {
            current: 1,
            expected_files: HashMap::new(),
        }
    }

    pub(super) const fn current(&self) -> u32 {
        self.current
    }

    pub(super) const fn begin_reload(&self) -> RevisionTicket {
        RevisionTicket(self.current)
    }

    pub(super) fn commit_reload(&self, ticket: RevisionTicket) -> Option<Self> {
        (ticket.0 == self.current).then(|| Self {
            current: self.current.saturating_add(1),
            expected_files: HashMap::new(),
        })
    }

    pub(super) fn commit_internal_write(
        &mut self,
        project_root: &Path,
        paths: impl IntoIterator<Item = impl AsRef<Path>>,
    ) {
        let mut revisions = Vec::new();
        for path in paths {
            let path = resolve_path(project_root, path.as_ref());
            let revision = match content_revision(&path) {
                Ok(revision) => ExpectedFileRevision::Present(revision),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                    ExpectedFileRevision::Missing
                }
                Err(_) => continue,
            };
            revisions.push((path, revision));
        }
        self.current = self.current.saturating_add(1);
        self.expected_files.extend(revisions);
    }

    pub(super) fn has_external_change(&self, project_root: &Path, paths: &[PathBuf]) -> bool {
        paths.iter().any(|path| {
            let path = resolve_path(project_root, path);
            // Recursive watchers may report the containing directory in the
            // same batch as a file write. Directory metadata does not affect
            // the compiled project, while real file create/delete events are
            // still represented by their file paths and remain external.
            if path.is_dir() {
                return false;
            }
            let Some(expected) = self.expected_files.get(&path) else {
                return true;
            };
            match expected {
                ExpectedFileRevision::Present(expected) => {
                    content_revision(&path).ok().as_ref() != Some(expected)
                }
                ExpectedFileRevision::Missing => path.exists(),
            }
        })
    }
}

fn resolve_path(project_root: &Path, path: &Path) -> PathBuf {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        project_root.join(path)
    };
    fs::canonicalize(&path).unwrap_or(path)
}

fn content_revision(path: &Path) -> std::io::Result<ContentRevision> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(ContentRevision(hasher.finalize().into()))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    #[test]
    fn exact_internal_content_is_not_external_but_later_content_is() {
        let root = std::env::temp_dir().join(format!(
            "coflow-editor-revision-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create temp directory");
        let path = root.join("data.cfd");
        fs::write(&path, "internal").expect("write internal content");

        let mut coordinator = RevisionCoordinator::initial();
        coordinator.commit_internal_write(&root, [Path::new("data.cfd")]);
        assert!(!coordinator.has_external_change(&root, std::slice::from_ref(&path)));
        assert!(!coordinator.has_external_change(&root, std::slice::from_ref(&root)));
        assert!(!coordinator.has_external_change(&root, &[root.clone(), path.clone()]));

        fs::write(&path, "external").expect("write external content");
        assert!(coordinator.has_external_change(&root, std::slice::from_ref(&path)));

        fs::remove_dir_all(root).expect("remove temp directory");
    }

    #[test]
    fn reload_ticket_only_advances_the_revision_that_created_it() {
        let mut coordinator = RevisionCoordinator::initial();
        let ticket = coordinator.begin_reload();
        let root = std::env::temp_dir();
        let path = root.join(format!("coflow-editor-ticket-{}", std::process::id()));
        fs::write(&path, "newer").expect("write revision fixture");
        coordinator.commit_internal_write(&root, [path.as_path()]);

        assert!(coordinator.commit_reload(ticket).is_none());
        fs::remove_file(path).expect("remove revision fixture");
    }

    #[test]
    fn internally_deleted_file_is_ignored_but_recreation_is_external() {
        let root = std::env::temp_dir().join(format!(
            "coflow-editor-revision-delete-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create temp directory");
        let path = root.join("generated.csv");
        fs::write(&path, "generated").expect("write generated file");
        fs::remove_file(&path).expect("remove generated file");

        let mut coordinator = RevisionCoordinator::initial();
        coordinator.commit_internal_write(&root, [path.as_path()]);
        assert!(!coordinator.has_external_change(&root, std::slice::from_ref(&path)));

        fs::write(&path, "external").expect("recreate generated file");
        assert!(coordinator.has_external_change(&root, std::slice::from_ref(&path)));
        fs::remove_dir_all(root).expect("remove temp directory");
    }
}
