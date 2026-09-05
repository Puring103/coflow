use crate::api::DiagnosticSet;
use crate::project::file_discovery::{
    discover_directory_files_with, DirectoryDiscoveryError, DirectoryDiscoveryErrorKind,
};
use crate::project::schema_path_policy::{SchemaFile, SchemaPathPolicy};
use crate::project::SchemaConfig;
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

#[derive(Debug, Default)]
struct SchemaDiscovery {
    files: Vec<SchemaFile>,
    visited_files: BTreeSet<PathBuf>,
}

pub(super) fn schema_files(
    schema: &SchemaConfig,
    root_dir: &Path,
) -> Result<Vec<SchemaFile>, DiagnosticSet> {
    let mut discovery = SchemaDiscovery::default();
    let mut errors = DiagnosticSet::empty();
    let policy = SchemaPathPolicy::new(root_dir);
    for path in schema.paths() {
        if let Err(err) = push_schema_path(policy, path, &mut discovery) {
            errors.extend(err);
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
) -> Result<Vec<SchemaSource>, DiagnosticSet> {
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
    Ok(modules)
}

fn push_schema_path(
    policy: SchemaPathPolicy<'_>,
    path: &Path,
    discovery: &mut SchemaDiscovery,
) -> Result<(), DiagnosticSet> {
    let path = policy.resolve(path);
    if path.is_dir() {
        let files = discover_directory_files_with(&path, &SchemaPathPolicy::is_cft_path)
            .map_err(|error| schema_discovery_error(policy, &error))?;
        for file in files {
            if discovery.visited_files.insert(file.canonical_path.clone()) {
                discovery
                    .files
                    .push(policy.schema_file_with_identity(file.path, file.canonical_path));
            }
        }
        Ok(())
    } else if path.is_file() {
        if !SchemaPathPolicy::is_cft_path(&path) {
            return Err(policy.unsupported_file_error(&path));
        }
        push_schema_file(policy, path, discovery)
    } else {
        Err(SchemaPathPolicy::missing_path_error(&path))
    }
}

fn push_schema_file(
    policy: SchemaPathPolicy<'_>,
    path: PathBuf,
    discovery: &mut SchemaDiscovery,
) -> Result<(), DiagnosticSet> {
    let canonical_path = SchemaPathPolicy::canonicalize(&path)?;
    if discovery.visited_files.insert(canonical_path.clone()) {
        discovery
            .files
            .push(policy.schema_file_with_identity(path, canonical_path));
    }
    Ok(())
}

fn schema_discovery_error(
    policy: SchemaPathPolicy<'_>,
    error: &DirectoryDiscoveryError,
) -> DiagnosticSet {
    match error.kind() {
        DirectoryDiscoveryErrorKind::NotDirectory { path } => {
            SchemaPathPolicy::missing_path_error(path)
        }
        DirectoryDiscoveryErrorKind::Resolve { path, message } => {
            SchemaPathPolicy::resolve_error(path, message)
        }
        DirectoryDiscoveryErrorKind::Read {
            path,
            operation,
            message,
        } => SchemaPathPolicy::read_dir_error(path, format!("{operation}: {message}")),
        DirectoryDiscoveryErrorKind::OutsideRoot {
            path,
            canonical_root,
            canonical_path,
        } => policy.outside_declared_root_error(path, canonical_root, canonical_path),
    }
}
