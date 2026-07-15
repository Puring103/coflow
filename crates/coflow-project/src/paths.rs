use crate::diagnostics::plain_error;
use coflow_api::DiagnosticSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

/// Resolves a config file path from an explicit path, directory, or current directory.
///
/// # Errors
///
/// Returns an error when the requested config file/directory cannot be resolved
/// to `coflow.yaml` or `coflow.yml`.
pub fn resolve_config_path(config_or_dir: Option<&Path>) -> Result<PathBuf, DiagnosticSet> {
    let candidate = config_or_dir.unwrap_or_else(|| Path::new("."));
    if config_or_dir.is_some() && candidate.is_file() {
        return Ok(candidate.to_path_buf());
    }
    if config_or_dir.is_some() && !candidate.exists() {
        if is_yaml_path(candidate) {
            return Ok(candidate.to_path_buf());
        }
        return Err(plain_error(
            "PROJECT-CONFIG-NOT-FOUND",
            "PROJECT",
            format!(
                "config or directory `{}` does not exist",
                candidate.display()
            ),
        ));
    }
    let dir = if candidate.is_dir() {
        candidate
    } else if is_yaml_path(candidate) {
        return Ok(candidate.to_path_buf());
    } else {
        return Err(plain_error(
            "PROJECT-CONFIG-PATH",
            "PROJECT",
            format!(
                "`{}` is neither a config file nor a directory",
                candidate.display()
            ),
        ));
    };
    find_default_config(dir)
}

pub(super) fn resolve_project_relative(root_dir: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root_dir.join(path)
    }
}

fn find_default_config(dir: &Path) -> Result<PathBuf, DiagnosticSet> {
    let yaml_path = dir.join("coflow.yaml");
    let yml_path = dir.join("coflow.yml");
    match (yaml_path.exists(), yml_path.exists()) {
        (true, false) => Ok(yaml_path),
        (false, true) => Ok(yml_path),
        (true, true) => Err(plain_error(
            "PROJECT-CONFIG-AMBIGUOUS",
            "PROJECT",
            format!(
                "both `{}` and `{}` exist; specify the config file explicitly",
                yaml_path.display(),
                yml_path.display()
            ),
        )),
        (false, false) => Err(plain_error(
            "PROJECT-CONFIG-NOT-FOUND",
            "PROJECT",
            format!("no coflow.yaml or coflow.yml found in `{}`", dir.display()),
        )),
    }
}

fn is_yaml_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| matches!(ext, "yaml" | "yml"))
}

#[must_use]
pub fn normalize_path(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| {
        let mut out = PathBuf::new();
        for component in path.components() {
            match component {
                Component::CurDir => {}
                Component::ParentDir => {
                    out.pop();
                }
                other => out.push(other.as_os_str()),
            }
        }
        out
    })
}

/// Returns a stable identity for ownership comparisons.
///
/// Existing paths are canonicalized first. Windows identities are folded to
/// lowercase because distinct CFT identifiers can otherwise target the same
/// case-insensitive filesystem entry.
#[must_use]
pub fn normalized_path_identity(path: &Path) -> String {
    let normalized = normalize_path(path);
    let identity = coflow_api::path_to_slash(&normalized);
    if cfg!(windows) {
        identity.to_lowercase()
    } else {
        identity
    }
}

#[must_use]
pub fn path_is_same_or_descendant(path: &Path, root: &Path) -> bool {
    let path = normalized_path_identity(path);
    let mut root = normalized_path_identity(root);
    while root.ends_with('/') && root.len() > 1 {
        root.pop();
    }
    if root == "/" {
        return path.starts_with('/');
    }
    path == root
        || path
            .strip_prefix(&root)
            .is_some_and(|suffix| suffix.starts_with('/'))
}
