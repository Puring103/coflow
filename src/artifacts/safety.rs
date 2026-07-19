use coflow_api::{Diagnostic, DiagnosticSet, Label, Severity, SourceLocation, SourceLocationSpec};
use coflow_project::Project;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub(crate) struct ArtifactOutputPlan {
    label: String,
    dir: PathBuf,
}

impl ArtifactOutputPlan {
    pub(crate) fn new(label: impl Into<String>, dir: PathBuf) -> Self {
        Self {
            label: label.into(),
            dir,
        }
    }
}

pub(crate) fn artifact_safety_diagnostics(
    project: &Project,
    outputs: &[ArtifactOutputPlan],
) -> DiagnosticSet {
    let mut diagnostics = DiagnosticSet::empty();
    let mut resolved_outputs = Vec::new();
    for output in outputs {
        if output.dir.exists() && !output.dir.is_dir() {
            diagnostics.push(artifact_diagnostic(
                &output.dir,
                format!(
                    "output dir `{}` already exists and is not a directory",
                    output.dir.display()
                ),
            ));
        }
        match normalized_existing_or_future_path(&output.dir) {
            Ok(output_dir) => {
                diagnostics.extend(output_scope_diagnostics(project, output, &output_dir));
                resolved_outputs.push((output, output_dir));
            }
            Err(err) => diagnostics.push(artifact_diagnostic(
                &output.dir,
                format!(
                    "failed to resolve existing ancestor of {} `{}`: {err}",
                    output.label,
                    output.dir.display()
                ),
            )),
        }
    }
    diagnostics.extend(overlapping_output_diagnostics(&resolved_outputs));
    diagnostics
}

fn output_scope_diagnostics(
    project: &Project,
    output: &ArtifactOutputPlan,
    output_dir: &Path,
) -> DiagnosticSet {
    let mut diagnostics = DiagnosticSet::empty();

    let project_root =
        resolve_input_path(&project.root_dir, "project root", output, &mut diagnostics);
    if project_root.as_deref() == Some(output_dir) {
        diagnostics.push(artifact_diagnostic(
            &output.dir,
            format!(
                "{} `{}` overlaps the project root; choose a dedicated generated output directory",
                output.label,
                output.dir.display()
            ),
        ));
    }

    let config_path = resolve_input_path(
        &project.config_path,
        "project config",
        output,
        &mut diagnostics,
    );
    if config_path
        .as_deref()
        .is_some_and(|path| paths_overlap(output_dir, path))
    {
        diagnostics.push(artifact_diagnostic(
            &output.dir,
            format!(
                "{} `{}` overlaps project config `{}`",
                output.label,
                output.dir.display(),
                project.config_path.display()
            ),
        ));
    }

    for schema_path in configured_schema_paths(project) {
        let resolved = resolve_input_path(&schema_path, "schema path", output, &mut diagnostics);
        if resolved
            .as_deref()
            .is_some_and(|path| paths_overlap(output_dir, path))
        {
            diagnostics.push(artifact_diagnostic(
                &output.dir,
                format!(
                    "{} `{}` overlaps schema path `{}`",
                    output.label,
                    output.dir.display(),
                    schema_path.display()
                ),
            ));
        }
    }

    for source_path in configured_source_paths(project) {
        let resolved = resolve_input_path(&source_path, "data source", output, &mut diagnostics);
        if resolved
            .as_deref()
            .is_some_and(|path| paths_overlap(output_dir, path))
        {
            diagnostics.push(artifact_diagnostic(
                &output.dir,
                format!(
                    "{} `{}` overlaps data source `{}`",
                    output.label,
                    output.dir.display(),
                    source_path.display()
                ),
            ));
        }
    }

    diagnostics
}

fn resolve_input_path(
    path: &Path,
    label: &str,
    output: &ArtifactOutputPlan,
    diagnostics: &mut DiagnosticSet,
) -> Option<PathBuf> {
    match normalized_existing_or_future_path(path) {
        Ok(path) => Some(path),
        Err(err) => {
            diagnostics.push(artifact_diagnostic(
                &output.dir,
                format!(
                    "cannot verify {} `{}` against {label} `{}`: {err}",
                    output.label,
                    output.dir.display(),
                    path.display()
                ),
            ));
            None
        }
    }
}

fn overlapping_output_diagnostics(outputs: &[(&ArtifactOutputPlan, PathBuf)]) -> DiagnosticSet {
    let mut diagnostics = DiagnosticSet::empty();
    for (index, left) in outputs.iter().enumerate() {
        for right in outputs.iter().skip(index + 1) {
            if paths_overlap(&left.1, &right.1) {
                diagnostics.push(artifact_diagnostic(
                    &left.0.dir,
                    format!(
                        "{} `{}` and {} `{}` overlap; choose separate generated output directories",
                        left.0.label,
                        left.0.dir.display(),
                        right.0.label,
                        right.0.dir.display()
                    ),
                ));
            }
        }
    }
    diagnostics
}

fn configured_schema_paths(project: &Project) -> Vec<PathBuf> {
    match &project.config.schema {
        coflow_project::SchemaConfig::One(path) => vec![project.resolve_path(path)],
        coflow_project::SchemaConfig::Many(paths) => paths
            .iter()
            .map(|path| project.resolve_path(path))
            .collect(),
    }
}

fn configured_source_paths(project: &Project) -> Vec<PathBuf> {
    project
        .config
        .sources
        .iter()
        .map(|source| match source.location() {
            SourceLocationSpec::Path(path) => path,
        })
        .flat_map(|path| source_overlap_paths(&project.resolve_path(path)))
        .collect()
}

fn source_overlap_paths(path: &Path) -> Vec<PathBuf> {
    let mut paths = vec![path.to_path_buf()];
    let is_file_source = fs::metadata(path).map_or_else(
        |_| path.extension().is_some(),
        |metadata| metadata.is_file(),
    );
    if is_file_source {
        if let Some(parent) = path.parent() {
            paths.push(parent.to_path_buf());
        }
    }
    paths
}

fn normalized_existing_or_future_path(path: &Path) -> io::Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let absolute = normalize_path_lexically(&absolute);
    let mut ancestor = absolute.as_path();
    let mut missing_components = Vec::new();

    loop {
        match fs::symlink_metadata(ancestor) {
            Ok(_) => {
                let mut resolved = fs::canonicalize(ancestor)?;
                for component in missing_components.iter().rev() {
                    resolved.push(component);
                }
                return Ok(resolved);
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                let Some(component) = ancestor.file_name() else {
                    return Err(err);
                };
                missing_components.push(component.to_os_string());
                let Some(parent) = ancestor.parent() else {
                    return Err(err);
                };
                ancestor = parent;
            }
            Err(err) => return Err(err),
        }
    }
}

fn normalize_path_lexically(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            _ => out.push(component.as_os_str()),
        }
    }
    out
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

pub(super) fn artifact_diagnostic(path: &Path, message: impl Into<String>) -> Diagnostic {
    Diagnostic {
        code: "ARTIFACT-001".to_string(),
        stage: "ARTIFACT".to_string(),
        severity: Severity::Error,
        message: message.into(),
        primary: Some(Label {
            location: SourceLocation::Artifact {
                path: path.to_path_buf(),
            },
            message: None,
        }),
        related: Vec::new(),
    }
}

pub(crate) fn artifact_diagnostic_set(path: &Path, message: impl Into<String>) -> DiagnosticSet {
    DiagnosticSet::one(artifact_diagnostic(path, message))
}
