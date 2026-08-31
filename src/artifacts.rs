use atomicwrites::{AllowOverwrite, AtomicFile};
use coflow_runtime::codegen::CodeArtifactFile;
use coflow_runtime::{Diagnostic, DiagnosticSet, Label, Project, SourceLocation};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const MANIFEST_VERSION: u32 = 1;
const STATE_DIR: &str = ".coflow/artifacts";
const ACTIVE_MANIFEST: &str = "active.json";
static REVISION_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone)]
pub(crate) struct CodeOutput {
    pub(crate) slot: String,
    pub(crate) directory: PathBuf,
    pub(crate) files: Vec<CodeArtifactFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct PublishedOutput {
    requested_dir: PathBuf,
    generation_dir: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ArtifactManifest {
    version: u32,
    revision: String,
    outputs: BTreeMap<String, PublishedOutput>,
}

#[derive(Debug)]
pub(crate) struct PreparedCodeRelease<'a> {
    project: &'a Project,
    outputs: Vec<CodeOutput>,
    active: Option<ArtifactManifest>,
}

impl<'a> PreparedCodeRelease<'a> {
    pub(crate) fn new(
        project: &'a Project,
        outputs: Vec<CodeOutput>,
    ) -> Result<Self, DiagnosticSet> {
        validate_outputs(project, &outputs)?;
        let active = load_manifest(project)?;
        Ok(Self {
            project,
            outputs,
            active,
        })
    }

    pub(crate) fn has_changes(&self) -> Result<bool, DiagnosticSet> {
        let Some(active) = &self.active else {
            return Ok(true);
        };
        let slots = self
            .outputs
            .iter()
            .map(|output| output.slot.as_str())
            .collect::<BTreeSet<_>>();
        if active.outputs.len() != slots.len()
            || active
                .outputs
                .keys()
                .any(|slot| !slots.contains(slot.as_str()))
        {
            return Ok(true);
        }
        for output in &self.outputs {
            let Some(published) = active.outputs.get(&output.slot) else {
                return Ok(true);
            };
            if published.requested_dir != output.directory
                || !artifact_files_match(&output.files, &output.directory)?
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub(crate) fn publish(self) -> Result<(), DiagnosticSet> {
        if !self.has_changes()? {
            return Ok(());
        }
        let generations = artifact_state_dir(self.project).join("generations");
        fs::create_dir_all(&generations).map_err(|error| {
            artifact_error(
                &generations,
                format!("failed to create artifact history: {error}"),
            )
        })?;

        let mut staged = Vec::with_capacity(self.outputs.len());
        for output in &self.outputs {
            staged.push(StagedOutput::create(&generations, output)?);
        }
        let mut published_count = 0;
        for output in &mut staged {
            if let Err(error) = output.publish_requested() {
                rollback_requested(&mut staged, published_count);
                return Err(error);
            }
            published_count += 1;
        }

        let manifest = ArtifactManifest {
            version: MANIFEST_VERSION,
            revision: unique_revision(),
            outputs: staged
                .iter()
                .map(|output| {
                    (
                        output.slot.clone(),
                        PublishedOutput {
                            requested_dir: output.requested_dir.clone(),
                            generation_dir: output.generation_dir.clone(),
                        },
                    )
                })
                .collect(),
        };
        if let Err(error) = write_manifest(self.project, &manifest) {
            rollback_requested(&mut staged, published_count);
            return Err(error);
        }
        for output in &mut staged {
            output.activate();
        }
        Ok(())
    }
}

#[derive(Debug)]
struct StagedOutput {
    slot: String,
    requested_dir: PathBuf,
    requested_staging: PathBuf,
    requested_backup: Option<PathBuf>,
    generation_dir: PathBuf,
    published: bool,
    active: bool,
}

impl StagedOutput {
    fn create(generations: &Path, output: &CodeOutput) -> Result<Self, DiagnosticSet> {
        let generation_dir = unique_child(generations, &output.slot);
        fs::create_dir(&generation_dir).map_err(|error| {
            artifact_error(
                &generation_dir,
                format!("failed to create artifact generation: {error}"),
            )
        })?;
        if let Err(error) = write_artifacts(&generation_dir, &output.files) {
            let _ = fs::remove_dir_all(&generation_dir);
            return Err(error);
        }

        let parent = output.directory.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent).map_err(|error| {
            artifact_error(parent, format!("failed to create output parent: {error}"))
        })?;
        let requested_staging = unique_sibling(&output.directory, "staging");
        fs::create_dir(&requested_staging).map_err(|error| {
            artifact_error(
                &requested_staging,
                format!("failed to create output staging directory: {error}"),
            )
        })?;
        if let Err(error) = write_artifacts(&requested_staging, &output.files)
            .and_then(|()| preserve_unity_meta(&output.directory, &requested_staging))
        {
            let _ = fs::remove_dir_all(&generation_dir);
            let _ = fs::remove_dir_all(&requested_staging);
            return Err(error);
        }
        Ok(Self {
            slot: output.slot.clone(),
            requested_dir: output.directory.clone(),
            requested_staging,
            requested_backup: None,
            generation_dir,
            published: false,
            active: false,
        })
    }

    fn publish_requested(&mut self) -> Result<(), DiagnosticSet> {
        if self.requested_dir.exists() {
            if !self.requested_dir.is_dir() {
                return Err(artifact_error(
                    &self.requested_dir,
                    "codegen output exists and is not a directory",
                ));
            }
            let backup = unique_sibling(&self.requested_dir, "backup");
            fs::rename(&self.requested_dir, &backup).map_err(|error| {
                artifact_error(
                    &self.requested_dir,
                    format!("failed to back up existing output: {error}"),
                )
            })?;
            self.requested_backup = Some(backup);
        }
        if let Err(error) = fs::rename(&self.requested_staging, &self.requested_dir) {
            self.restore();
            return Err(artifact_error(
                &self.requested_dir,
                format!("failed to publish generated output: {error}"),
            ));
        }
        self.published = true;
        Ok(())
    }

    fn restore(&mut self) {
        if self.published && self.requested_dir.is_dir() {
            let _ = fs::remove_dir_all(&self.requested_dir);
        }
        if let Some(backup) = self.requested_backup.take() {
            let _ = fs::rename(backup, &self.requested_dir);
        }
        self.published = false;
    }

    fn activate(&mut self) {
        self.active = true;
        if let Some(backup) = self.requested_backup.take() {
            let _ = fs::remove_dir_all(backup);
        }
    }
}

impl Drop for StagedOutput {
    fn drop(&mut self) {
        if !self.active {
            self.restore();
            let _ = fs::remove_dir_all(&self.generation_dir);
        }
        if self.requested_staging.is_dir() {
            let _ = fs::remove_dir_all(&self.requested_staging);
        }
    }
}

fn rollback_requested(outputs: &mut [StagedOutput], published_count: usize) {
    for output in outputs[..published_count].iter_mut().rev() {
        output.restore();
    }
}

pub(crate) fn clean_history(project: &Project) -> Result<(usize, usize), DiagnosticSet> {
    let active = load_manifest(project)?
        .into_iter()
        .flat_map(|manifest| manifest.outputs.into_values())
        .map(|output| output.generation_dir)
        .collect::<BTreeSet<_>>();
    let state = artifact_state_dir(project);
    let generations = clean_children(&state.join("generations"), &active)?;
    let staging = clean_children(&state.join("staging"), &BTreeSet::new())?;
    Ok((generations, staging))
}

fn clean_children(parent: &Path, preserved: &BTreeSet<PathBuf>) -> Result<usize, DiagnosticSet> {
    if !parent.exists() {
        return Ok(0);
    }
    let entries = fs::read_dir(parent).map_err(|error| {
        artifact_error(parent, format!("failed to read artifact state: {error}"))
    })?;
    let mut removed = 0;
    for entry in entries {
        let entry = entry.map_err(|error| {
            artifact_error(parent, format!("failed to read artifact state: {error}"))
        })?;
        let path = entry.path();
        if preserved.contains(&path) {
            continue;
        }
        let file_type = entry.file_type().map_err(|error| {
            artifact_error(&path, format!("failed to inspect artifact state: {error}"))
        })?;
        if file_type.is_dir() {
            fs::remove_dir_all(&path)
        } else {
            fs::remove_file(&path)
        }
        .map_err(|error| {
            artifact_error(&path, format!("failed to remove artifact state: {error}"))
        })?;
        removed += 1;
    }
    Ok(removed)
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
        ("artifact state", artifact_state_dir(project)),
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

fn load_manifest(project: &Project) -> Result<Option<ArtifactManifest>, DiagnosticSet> {
    let path = manifest_path(project);
    if !path.exists() {
        return Ok(None);
    }
    let manifest: ArtifactManifest = serde_json::from_slice(&fs::read(&path).map_err(|error| {
        artifact_error(&path, format!("failed to read active manifest: {error}"))
    })?)
    .map_err(|error| artifact_error(&path, format!("failed to parse active manifest: {error}")))?;
    if manifest.version != MANIFEST_VERSION {
        return Err(artifact_error(
            &path,
            format!("unsupported active manifest version `{}`", manifest.version),
        ));
    }
    for (slot, output) in &manifest.outputs {
        if !output.generation_dir.is_dir() {
            return Err(artifact_error(
                &path,
                format!("active `{slot}` artifact generation is missing"),
            ));
        }
    }
    Ok(Some(manifest))
}

fn write_manifest(project: &Project, manifest: &ArtifactManifest) -> Result<(), DiagnosticSet> {
    let path = manifest_path(project);
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|error| {
        artifact_error(parent, format!("failed to create artifact state: {error}"))
    })?;
    let contents = serde_json::to_vec_pretty(manifest).map_err(|error| {
        artifact_error(
            &path,
            format!("failed to serialize active manifest: {error}"),
        )
    })?;
    AtomicFile::new(&path, AllowOverwrite)
        .write(|file| file.write_all(&contents))
        .map_err(|error| {
            artifact_error(&path, format!("failed to publish active manifest: {error}"))
        })
}

fn artifact_state_dir(project: &Project) -> PathBuf {
    project.root_dir().join(STATE_DIR)
}

fn manifest_path(project: &Project) -> PathBuf {
    artifact_state_dir(project).join(ACTIVE_MANIFEST)
}

fn unique_child(parent: &Path, slot: &str) -> PathBuf {
    let safe_slot = slot
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    parent.join(format!("{safe_slot}-{}", unique_revision()))
}

fn unique_sibling(path: &Path, kind: &str) -> PathBuf {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("generated");
    parent.join(format!(".{name}.coflow-{kind}-{}", unique_revision()))
}

fn unique_revision() -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let sequence = REVISION_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!("{}-{timestamp}-{sequence}", std::process::id())
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

fn artifact_error(path: &Path, message: impl Into<String>) -> DiagnosticSet {
    DiagnosticSet::one(artifact_diagnostic(path, message))
}
