use crate::schema_path_policy::{SchemaFile, SchemaPathPolicy};
use crate::SchemaConfig;
use coflow_api::DiagnosticSet;
use std::fs;
use std::path::Path;

pub(super) fn schema_files(
    schema: &SchemaConfig,
    root_dir: &Path,
) -> Result<Vec<SchemaFile>, DiagnosticSet> {
    let mut files = Vec::new();
    let mut errors = DiagnosticSet::empty();
    let policy = SchemaPathPolicy::new(root_dir);
    match schema {
        SchemaConfig::One(path) => {
            if let Err(err) = push_schema_path(policy, path, &mut files) {
                errors.extend(err);
            }
        }
        SchemaConfig::Many(paths) => {
            for path in paths {
                if let Err(err) = push_schema_path(policy, path, &mut files) {
                    errors.extend(err);
                }
            }
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    files.sort_by(|left, right| left.module_id.cmp(&right.module_id));
    Ok(files)
}

fn push_schema_path(
    policy: SchemaPathPolicy<'_>,
    path: &Path,
    files: &mut Vec<SchemaFile>,
) -> Result<(), DiagnosticSet> {
    let path = policy.resolve(path);
    if path.is_dir() {
        collect_cft_files(policy, &path, files)
    } else if path.is_file() {
        if !SchemaPathPolicy::is_cft_path(&path) {
            return Err(policy.unsupported_file_error(&path));
        }
        files.push(policy.schema_file(path)?);
        Ok(())
    } else {
        Err(policy.missing_path_error(&path))
    }
}

fn collect_cft_files(
    policy: SchemaPathPolicy<'_>,
    dir: &Path,
    files: &mut Vec<SchemaFile>,
) -> Result<(), DiagnosticSet> {
    let mut entries = fs::read_dir(dir)
        .map_err(|err| policy.read_dir_error(dir, err))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| policy.read_dir_error(dir, err))?;
    entries.sort_by_key(fs::DirEntry::path);
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            collect_cft_files(policy, &path, files)?;
        } else if SchemaPathPolicy::is_cft_path(&path) {
            files.push(policy.schema_file(path)?);
        }
    }
    Ok(())
}
