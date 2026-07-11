use crate::schema_path_policy::{SchemaFile, SchemaPathPolicy};
use crate::SchemaConfig;
use coflow_api::DiagnosticSet;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaSource {
    pub module_id: String,
    pub path: PathBuf,
    pub canonical_path: PathBuf,
    pub source: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SchemaSourceSet {
    pub modules: Vec<SchemaSource>,
}

#[derive(Debug, Default)]
struct SchemaDiscovery {
    files: Vec<SchemaFile>,
    visited_directories: BTreeSet<PathBuf>,
    visited_files: BTreeSet<PathBuf>,
}

pub(super) fn schema_files(
    schema: &SchemaConfig,
    root_dir: &Path,
) -> Result<Vec<SchemaFile>, DiagnosticSet> {
    let mut discovery = SchemaDiscovery::default();
    let mut errors = DiagnosticSet::empty();
    let policy = SchemaPathPolicy::new(root_dir);
    match schema {
        SchemaConfig::One(path) => {
            if let Err(err) = push_schema_path(policy, path, &mut discovery) {
                errors.extend(err);
            }
        }
        SchemaConfig::Many(paths) => {
            for path in paths {
                if let Err(err) = push_schema_path(policy, path, &mut discovery) {
                    errors.extend(err);
                }
            }
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    discovery
        .files
        .sort_by(|left, right| left.module_id.cmp(&right.module_id));
    Ok(discovery.files)
}

pub(super) fn schema_sources(
    schema: &SchemaConfig,
    root_dir: &Path,
) -> Result<SchemaSourceSet, DiagnosticSet> {
    let files = schema_files(schema, root_dir)?;
    let mut modules = Vec::with_capacity(files.len());
    for file in files {
        let source = fs::read_to_string(&file.path)
            .map_err(|err| SchemaPathPolicy::read_file_error(&file.path, err))?;
        modules.push(SchemaSource {
            module_id: file.module_id,
            path: file.path,
            canonical_path: file.canonical_path,
            source,
        });
    }
    Ok(SchemaSourceSet { modules })
}

fn push_schema_path(
    policy: SchemaPathPolicy<'_>,
    path: &Path,
    discovery: &mut SchemaDiscovery,
) -> Result<(), DiagnosticSet> {
    let path = policy.resolve(path);
    if path.is_dir() {
        let canonical_root = SchemaPathPolicy::canonicalize(&path)?;
        collect_cft_files(policy, &path, &canonical_root, discovery)
    } else if path.is_file() {
        if !SchemaPathPolicy::is_cft_path(&path) {
            return Err(policy.unsupported_file_error(&path));
        }
        push_schema_file(policy, path, None, discovery)
    } else {
        Err(SchemaPathPolicy::missing_path_error(&path))
    }
}

fn collect_cft_files(
    policy: SchemaPathPolicy<'_>,
    dir: &Path,
    canonical_root: &Path,
    discovery: &mut SchemaDiscovery,
) -> Result<(), DiagnosticSet> {
    let canonical_dir = SchemaPathPolicy::canonicalize(dir)?;
    if !canonical_dir.starts_with(canonical_root) {
        return Err(policy.outside_declared_root_error(dir, canonical_root, &canonical_dir));
    }
    if !discovery.visited_directories.insert(canonical_dir) {
        return Ok(());
    }

    let mut entries = fs::read_dir(dir)
        .map_err(|err| SchemaPathPolicy::read_dir_error(dir, err))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| SchemaPathPolicy::read_dir_error(dir, err))?;
    entries.sort_by_key(fs::DirEntry::path);
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            collect_cft_files(policy, &path, canonical_root, discovery)?;
        } else if SchemaPathPolicy::is_cft_path(&path) {
            push_schema_file(policy, path, Some(canonical_root), discovery)?;
        }
    }
    Ok(())
}

fn push_schema_file(
    policy: SchemaPathPolicy<'_>,
    path: PathBuf,
    canonical_root: Option<&Path>,
    discovery: &mut SchemaDiscovery,
) -> Result<(), DiagnosticSet> {
    let canonical_path = SchemaPathPolicy::canonicalize(&path)?;
    if let Some(canonical_root) = canonical_root {
        if !canonical_path.starts_with(canonical_root) {
            return Err(policy.outside_declared_root_error(&path, canonical_root, &canonical_path));
        }
    }
    if discovery.visited_files.insert(canonical_path.clone()) {
        discovery
            .files
            .push(policy.schema_file_with_identity(path, canonical_path));
    }
    Ok(())
}
