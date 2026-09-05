//! Tiny path helpers shared by every sub-module of the session.

pub(super) fn strip_unc_prefix(path: &str) -> String {
    path.strip_prefix(r"\\?\")
        .or_else(|| path.strip_prefix("//?/"))
        .map_or_else(|| path.to_owned(), str::to_owned)
}
