use crate::codegen::{CodeArtifactFile, IdAsEnumValues};
use crate::{Diagnostic, DiagnosticSet, Label, Project, SourceLocation};
use coflow_staging::{StagedChange, StagedDirectory, StagedFile};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

const ENUM_LOCKFILE_NAME: &str = "coflow.enum.lock.json";
const INTERNAL_STATE_DIR_NAME: &str = ".coflow";

#[derive(Debug, Clone)]
pub(crate) struct CodeOutput {
    pub(crate) slot: String,
    pub(crate) directory: PathBuf,
    pub(crate) files: Vec<CodeArtifactFile>,
}

#[derive(Debug)]
pub(crate) struct PreparedCodeRelease {
    outputs: Vec<CodeOutput>,
    id_as_enum_values: IdAsEnumValues,
    enum_lock_path: PathBuf,
    enum_lock_original: Option<Vec<u8>>,
    enum_lock_changed: bool,
}

impl PreparedCodeRelease {
    pub(crate) fn new(
        project: &Project,
        outputs: Vec<CodeOutput>,
        id_as_enum_values: IdAsEnumValues,
    ) -> Result<Self, DiagnosticSet> {
        validate_outputs(project, &outputs)?;
        let enum_lock_path = enum_lockfile_path(project);
        let enum_lock_original = read_optional_file(&enum_lock_path)?;
        let current_values =
            parse_id_as_enum_values(&enum_lock_path, enum_lock_original.as_deref())?;
        let enum_lock_changed = current_values != id_as_enum_values;
        Ok(Self {
            outputs,
            id_as_enum_values,
            enum_lock_path,
            enum_lock_original,
            enum_lock_changed,
        })
    }

    pub(crate) fn has_changes(&self) -> Result<bool, DiagnosticSet> {
        if self.enum_lock_changed {
            return Ok(true);
        }
        for output in &self.outputs {
            if !artifact_files_match(&output.files, &output.directory)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub(crate) fn publish(self) -> Result<(), DiagnosticSet> {
        if !self.has_changes()? {
            return Ok(());
        }
        let mut staged = Vec::with_capacity(self.outputs.len());
        for output in &self.outputs {
            let directory = StagedDirectory::create(&output.directory)
                .map_err(map_staging_error)?;
            write_artifacts(directory.staging(), &output.files)
                .and_then(|()| preserve_unity_meta(&output.directory, directory.staging()))?;
            staged.push(directory);
        }
        let mut staged_lock = if self.enum_lock_changed {
            Some(
                StagedFile::create(
                    &self.enum_lock_path,
                    self.enum_lock_original,
                    &serde_json::to_vec_pretty(&self.id_as_enum_values).map_err(|error| {
                        artifact_error(
                            &self.enum_lock_path,
                            format!("failed to serialize enum lock: {error}"),
                        )
                    })?,
                )
                .map_err(map_staging_error)?,
            )
        } else {
            None
        };
        let mut published_count = 0;
        for output in &mut staged {
            if let Err(error) = output.publish() {
                rollback_requested(&mut staged, published_count);
                return Err(map_staging_error(error));
            }
            published_count += 1;
        }

        if let Some(lock) = &mut staged_lock {
            if let Err(error) = lock.publish() {
                rollback_requested(&mut staged, published_count);
                return Err(map_staging_error(error));
            }
        }
        for output in &mut staged {
            output.finish();
        }
        if let Some(lock) = &mut staged_lock {
            lock.finish();
        }
        Ok(())
    }
}

fn rollback_requested(outputs: &mut [StagedDirectory], published_count: usize) {
    for output in outputs[..published_count].iter_mut().rev() {
        StagedChange::restore(output);
    }
}

fn map_staging_error(error: coflow_staging::StagingError) -> DiagnosticSet {
    artifact_error(error.path(), error.to_string())
}

fn validate_outputs(project: &Project, outputs: &[CodeOutput]) -> Result<(), DiagnosticSet> {
    let mut slots = BTreeSet::new();
    let mut diagnostics = DiagnosticSet::empty();
    let mut resolved_outputs = Vec::new();
    for output in outputs {
        if output.slot.is_empty() || !slots.insert(output.slot.as_str()) {
            diagnostics.push(artifact_diagnostic(
                &output.directory,
                format!("invalid or duplicate artifact slot `{}`", output.slot),
            ));
        }
        if output.directory.exists() && !output.directory.is_dir() {
            diagnostics.push(artifact_diagnostic(
                &output.directory,
                "codegen output exists and is not a directory",
            ));
        }
        match normalized_existing_or_future_path(&output.directory) {
            Ok(resolved) => {
                validate_output_scope(project, output, &resolved, &mut diagnostics);
                resolved_outputs.push((output, resolved));
            }
            Err(error) => diagnostics.push(artifact_diagnostic(
                &output.directory,
                format!(
                    "failed to resolve existing ancestor of codegen output `{}`: {error}",
                    output.directory.display()
                ),
            )),
        }
    }
    for (index, (left, left_path)) in resolved_outputs.iter().enumerate() {
        for (right, right_path) in resolved_outputs.iter().skip(index + 1) {
            if paths_overlap(left_path, right_path) {
                diagnostics.push(artifact_diagnostic(
                    &left.directory,
                    format!(
                        "codegen outputs `{}` and `{}` overlap",
                        left.directory.display(),
                        right.directory.display()
                    ),
                ));
            }
        }
    }
    if diagnostics.is_empty() {
        Ok(())
    } else {
        Err(diagnostics)
    }
}

fn validate_output_scope(
    project: &Project,
    output: &CodeOutput,
    resolved_output: &Path,
    diagnostics: &mut DiagnosticSet,
) {
    match normalized_existing_or_future_path(project.root_dir()) {
        Ok(root) if root == resolved_output => diagnostics.push(artifact_diagnostic(
            &output.directory,
            "codegen output cannot be the project root",
        )),
        Ok(_) => {}
        Err(error) => diagnostics.push(artifact_diagnostic(
            &output.directory,
            format!("cannot verify codegen output against project root: {error}"),
        )),
    }
    let mut protected = vec![
        ("project config", project.config_path().to_path_buf()),
        ("enum lock", enum_lockfile_path(project)),
        (
            "internal state directory",
            project.root_dir().join(INTERNAL_STATE_DIR_NAME),
        ),
    ];
    protected.extend(
        project
            .config()
            .schema
            .paths()
            .iter()
            .map(|path| ("schema path", project.resolve_path(path))),
    );
    for source in project.data_paths() {
        let path = project.resolve_path(source.path());
        protected.push(("data source", path.clone()));
        let is_file = fs::metadata(&path).map_or_else(
            |_| path.extension().is_some(),
            |metadata| metadata.is_file(),
        );
        if is_file {
            if let Some(parent) = path.parent() {
                protected.push(("data source directory", parent.to_path_buf()));
            }
        }
    }
    protected.extend(
        project
            .config()
            .dimensions
            .values()
            .filter_map(|dimension| {
                dimension
                    .out_dir
                    .as_ref()
                    .map(|path| ("dimension output", project.resolve_path(path)))
            }),
    );

    for (label, path) in protected {
        match normalized_existing_or_future_path(&path) {
            Ok(protected_path) if paths_overlap(resolved_output, &protected_path) => {
                diagnostics.push(artifact_diagnostic(
                    &output.directory,
                    format!(
                        "codegen output `{}` overlaps {label} `{}`",
                        output.directory.display(),
                        path.display()
                    ),
                ));
            }
            Ok(_) => {}
            Err(error) => diagnostics.push(artifact_diagnostic(
                &output.directory,
                format!(
                    "cannot verify codegen output `{}` against {label} `{}`: {error}",
                    output.directory.display(),
                    path.display()
                ),
            )),
        }
    }
}

fn normalized_existing_or_future_path(path: &Path) -> io::Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let absolute = normalize_path_lexically(&absolute);
    let mut ancestor = absolute.as_path();
    let mut missing = Vec::new();
    loop {
        match fs::symlink_metadata(ancestor) {
            Ok(_) => {
                let mut resolved = fs::canonicalize(ancestor)?;
                for component in missing.iter().rev() {
                    resolved.push(component);
                }
                return Ok(resolved);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let Some(component) = ancestor.file_name() else {
                    return Err(error);
                };
                missing.push(component.to_os_string());
                let Some(parent) = ancestor.parent() else {
                    return Err(error);
                };
                ancestor = parent;
            }
            Err(error) => return Err(error),
        }
    }
}

fn normalize_path_lexically(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            _ => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

fn paths_overlap(left: &Path, right: &Path) -> bool {
    let left = windows_path_key(left);
    let right = windows_path_key(right);
    left == right || left.starts_with(&right) || right.starts_with(&left)
}

fn windows_path_key(path: &Path) -> Vec<String> {
    path.components()
        .map(|component| {
            component
                .as_os_str()
                .to_string_lossy()
                .trim_end_matches([' ', '.'])
                .to_lowercase()
        })
        .collect()
}

fn artifact_files_match(
    files: &[CodeArtifactFile],
    directory: &Path,
) -> Result<bool, DiagnosticSet> {
    if !directory.is_dir() {
        return Ok(false);
    }
    let mut expected = files
        .iter()
        .map(|file| (file.relative_path.clone(), file.contents.as_bytes()))
        .collect::<BTreeMap<_, _>>();
    Ok(compare_tree(directory, directory, &mut expected)? && expected.is_empty())
}

fn compare_tree<'a>(
    root: &Path,
    directory: &Path,
    expected: &mut BTreeMap<PathBuf, &'a [u8]>,
) -> Result<bool, DiagnosticSet> {
    let entries = fs::read_dir(directory)
        .map_err(|error| artifact_error(directory, format!("failed to inspect output: {error}")))?;
    for entry in entries {
        let entry = entry.map_err(|error| {
            artifact_error(directory, format!("failed to inspect output: {error}"))
        })?;
        let path = entry.path();
        let file_type = entry.file_type().map_err(|error| {
            artifact_error(&path, format!("failed to inspect output entry: {error}"))
        })?;
        if file_type.is_dir() {
            if !compare_tree(root, &path, expected)? {
                return Ok(false);
            }
            continue;
        }
        if !file_type.is_file() {
            return Ok(false);
        }
        let relative = path.strip_prefix(root).map_err(|error| {
            artifact_error(&path, format!("failed to resolve output entry: {error}"))
        })?;
        if is_unity_meta(&path) && !expected.contains_key(relative) {
            continue;
        }
        let Some(contents) = expected.remove(relative) else {
            return Ok(false);
        };
        if fs::read(&path).map_err(|error| {
            artifact_error(&path, format!("failed to read output entry: {error}"))
        })? != contents
        {
            return Ok(false);
        }
    }
    Ok(true)
}

fn write_artifacts(root: &Path, files: &[CodeArtifactFile]) -> Result<(), DiagnosticSet> {
    for file in files {
        let path = root.join(&file.relative_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                artifact_error(
                    parent,
                    format!("failed to create artifact directory: {error}"),
                )
            })?;
        }
        let mut output = fs::File::create(&path).map_err(|error| {
            artifact_error(&path, format!("failed to create artifact: {error}"))
        })?;
        output
            .write_all(file.contents.as_bytes())
            .map_err(|error| artifact_error(&path, format!("failed to write artifact: {error}")))?;
        output
            .sync_all()
            .map_err(|error| artifact_error(&path, format!("failed to sync artifact: {error}")))?;
    }
    Ok(())
}

fn preserve_unity_meta(source: &Path, destination: &Path) -> Result<(), DiagnosticSet> {
    if !source.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(source).map_err(|error| {
        artifact_error(source, format!("failed to inspect Unity metadata: {error}"))
    })? {
        let entry = entry.map_err(|error| {
            artifact_error(source, format!("failed to inspect Unity metadata: {error}"))
        })?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let file_type = entry.file_type().map_err(|error| {
            artifact_error(
                &source_path,
                format!("failed to inspect Unity metadata: {error}"),
            )
        })?;
        if file_type.is_dir() {
            if !destination_path.exists() {
                fs::create_dir_all(&destination_path).map_err(|error| {
                    artifact_error(
                        &destination_path,
                        format!("failed to stage Unity metadata directory: {error}"),
                    )
                })?;
            }
            if destination_path.is_dir() {
                preserve_unity_meta(&source_path, &destination_path)?;
            }
        } else if file_type.is_file() && is_unity_meta(&source_path) && !destination_path.exists() {
            fs::copy(&source_path, &destination_path).map_err(|error| {
                artifact_error(
                    &source_path,
                    format!("failed to preserve Unity metadata: {error}"),
                )
            })?;
        }
    }
    Ok(())
}

pub(crate) fn read_id_as_enum_values(project: &Project) -> Result<IdAsEnumValues, DiagnosticSet> {
    let path = enum_lockfile_path(project);
    let contents = read_optional_file(&path)?;
    parse_id_as_enum_values(&path, contents.as_deref())
}

fn read_optional_file(path: &Path) -> Result<Option<Vec<u8>>, DiagnosticSet> {
    match fs::read(path) {
        Ok(contents) => Ok(Some(contents)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(artifact_error(
            path,
            format!("failed to read file: {error}"),
        )),
    }
}

fn parse_id_as_enum_values(
    path: &Path,
    contents: Option<&[u8]>,
) -> Result<IdAsEnumValues, DiagnosticSet> {
    contents.map_or_else(
        || Ok(IdAsEnumValues::new()),
        |contents| {
            serde_json::from_slice(contents).map_err(|error| {
                artifact_error(path, format!("failed to parse enum lock: {error}"))
            })
        },
    )
}

pub(crate) fn enum_lockfile_path(project: &Project) -> PathBuf {
    project.root_dir().join(ENUM_LOCKFILE_NAME)
}

fn is_unity_meta(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("meta"))
}

fn artifact_diagnostic(path: &Path, message: impl Into<String>) -> Diagnostic {
    Diagnostic::error("ARTIFACT-001", "ARTIFACT", message).with_primary(Label {
        location: SourceLocation::Artifact {
            path: path.to_path_buf(),
        },
        message: None,
    })
}

pub(crate) fn artifact_error(path: &Path, message: impl Into<String>) -> DiagnosticSet {
    DiagnosticSet::one(artifact_diagnostic(path, message))
}
