use std::fs;
use std::path::{Component, Path, PathBuf};

/// Resolves a config file path from an explicit path, directory, or current directory.
///
/// # Errors
///
/// Returns an error when the requested config file/directory cannot be resolved
/// to `coflow.yaml` or `coflow.yml`.
pub fn resolve_config_path(config_or_dir: Option<&Path>) -> Result<PathBuf, String> {
    let candidate = config_or_dir.unwrap_or_else(|| Path::new("."));
    if config_or_dir.is_some() && candidate.is_file() {
        return Ok(candidate.to_path_buf());
    }
    if config_or_dir.is_some() && !candidate.exists() {
        if is_yaml_path(candidate) {
            return Ok(candidate.to_path_buf());
        }
        return Err(format!(
            "config or directory `{}` does not exist",
            candidate.display()
        ));
    }
    let dir = if candidate.is_dir() {
        candidate
    } else if is_yaml_path(candidate) {
        return Ok(candidate.to_path_buf());
    } else {
        return Err(format!(
            "`{}` is neither a config file nor a directory",
            candidate.display()
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

fn find_default_config(dir: &Path) -> Result<PathBuf, String> {
    let yaml_path = dir.join("coflow.yaml");
    let yml_path = dir.join("coflow.yml");
    match (yaml_path.exists(), yml_path.exists()) {
        (true, false) => Ok(yaml_path),
        (false, true) => Ok(yml_path),
        (true, true) => Err(format!(
            "both `{}` and `{}` exist; specify the config file explicitly",
            yaml_path.display(),
            yml_path.display()
        )),
        (false, false) => Err(format!(
            "no coflow.yaml or coflow.yml found in `{}`",
            dir.display()
        )),
    }
}

fn is_yaml_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| matches!(ext, "yaml" | "yml"))
}

#[must_use]
pub fn path_to_slash(path: &Path) -> String {
    let raw = path
        .components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy().replace('\\', "/")),
            Component::Prefix(prefix) => Some(prefix.as_os_str().to_string_lossy().to_string()),
            Component::RootDir | Component::CurDir => None,
            Component::ParentDir => Some("..".to_string()),
        })
        .collect::<Vec<_>>()
        .join("/");
    // Strip the Windows verbatim-path prefix (\\?\  or //?/) so the result
    // is portable and can be round-tripped through YAML or the LSP protocol.
    raw.strip_prefix(r"\\?\")
        .or_else(|| raw.strip_prefix("//?/"))
        .map_or_else(|| raw.clone(), str::to_owned)
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
