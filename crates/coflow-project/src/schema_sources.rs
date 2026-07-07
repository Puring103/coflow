use crate::{path_to_slash, SchemaConfig};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct SchemaFile {
    pub path: PathBuf,
    pub canonical_path: PathBuf,
    pub module_id: String,
}

impl SchemaFile {
    fn new(path: PathBuf, root_dir: &Path) -> Result<Self, String> {
        let canonical_path = fs::canonicalize(&path)
            .map_err(|err| format!("failed to resolve schema `{}`: {err}", path.display()))?;
        let module_path = canonical_path
            .strip_prefix(root_dir)
            .unwrap_or(canonical_path.as_path());
        let module_id = path_to_slash(module_path);
        Ok(Self {
            path,
            canonical_path,
            module_id,
        })
    }
}

pub(super) fn schema_files(
    schema: &SchemaConfig,
    root_dir: &Path,
) -> Result<Vec<SchemaFile>, String> {
    let mut files = Vec::new();
    let mut errors = Vec::new();
    match schema {
        SchemaConfig::One(path) => {
            if let Err(err) = push_schema_path(root_dir, path, &mut files) {
                errors.push(err);
            }
        }
        SchemaConfig::Many(paths) => {
            for path in paths {
                if let Err(err) = push_schema_path(root_dir, path, &mut files) {
                    errors.push(err);
                }
            }
        }
    }
    if !errors.is_empty() {
        return Err(errors.join("\n"));
    }
    files.sort_by(|left, right| left.module_id.cmp(&right.module_id));
    Ok(files)
}

fn push_schema_path(
    root_dir: &Path,
    path: &Path,
    files: &mut Vec<SchemaFile>,
) -> Result<(), String> {
    let path = resolve_project_relative(root_dir, path);
    if path.is_dir() {
        collect_cft_files(&path, files, root_dir)
    } else if path.is_file() {
        if !is_cft_path(&path) {
            return Err(format!(
                "schema file `{}` has unsupported extension",
                path_to_slash(path.strip_prefix(root_dir).unwrap_or(&path))
            ));
        }
        files.push(SchemaFile::new(path, root_dir)?);
        Ok(())
    } else {
        Err(format!("schema path `{}` does not exist", path.display()))
    }
}

fn resolve_project_relative(root_dir: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root_dir.join(path)
    }
}

fn is_cft_path(path: &Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()) == Some("cft")
}

fn collect_cft_files(
    dir: &Path,
    files: &mut Vec<SchemaFile>,
    root_dir: &Path,
) -> Result<(), String> {
    let mut entries = fs::read_dir(dir)
        .map_err(|err| format!("failed to read schema directory `{}`: {err}", dir.display()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| format!("failed to read schema directory `{}`: {err}", dir.display()))?;
    entries.sort_by_key(fs::DirEntry::path);
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            collect_cft_files(&path, files, root_dir)?;
        } else if is_cft_path(&path) {
            files.push(SchemaFile::new(path, root_dir)?);
        }
    }
    Ok(())
}
