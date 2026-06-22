//! Tiny path helpers shared by every sub-module of the session.
use std::path::Path;

pub(super) fn path_to_slash(path: &Path) -> String {
    strip_unc_prefix(&path.to_string_lossy().replace('\\', "/"))
}

pub(super) fn strip_unc_prefix(path: &str) -> String {
    path.strip_prefix(r"\\?\")
        .or_else(|| path.strip_prefix("//?/"))
        .map(str::to_owned)
        .unwrap_or_else(|| path.to_owned())
}
