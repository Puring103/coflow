use coflow_api::{Diagnostic, DiagnosticSet, Label, Severity, SourceLocation, SourceLocationSpec};
use coflow_project::Project;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub(super) struct ArtifactOutputPlan {
    label: &'static str,
    dir: PathBuf,
}

impl ArtifactOutputPlan {
    pub(super) const fn new(label: &'static str, dir: PathBuf) -> Self {
        Self { label, dir }
    }
}

pub(super) fn artifact_safety_diagnostics(
    project: &Project,
    outputs: &[ArtifactOutputPlan],
) -> DiagnosticSet {
    let mut diagnostics = DiagnosticSet::empty();
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
        diagnostics.extend(output_scope_diagnostics(project, output));
    }
    diagnostics.extend(overlapping_output_diagnostics(outputs));
    diagnostics
}

fn output_scope_diagnostics(project: &Project, output: &ArtifactOutputPlan) -> DiagnosticSet {
    let output_dir = normalized_existing_or_future_path(&output.dir);
    let project_root = normalized_existing_or_future_path(&project.root_dir);
    let mut diagnostics = DiagnosticSet::empty();

    if output_dir == project_root {
        diagnostics.push(artifact_diagnostic(
            &output.dir,
            format!(
                "{} `{}` overlaps the project root; choose a dedicated generated output directory",
                output.label,
                output.dir.display()
            ),
        ));
    }

    let config_path = normalized_existing_or_future_path(&project.config_path);
    if paths_overlap(&output_dir, &config_path) {
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
        let schema_path = normalized_existing_or_future_path(&schema_path);
        if paths_overlap(&output_dir, &schema_path) {
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
        let source_path = normalized_existing_or_future_path(&source_path);
        if paths_overlap(&output_dir, &source_path) {
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

fn overlapping_output_diagnostics(outputs: &[ArtifactOutputPlan]) -> DiagnosticSet {
    let mut diagnostics = DiagnosticSet::empty();
    for (index, left) in outputs.iter().enumerate() {
        let left_dir = normalized_existing_or_future_path(&left.dir);
        for right in outputs.iter().skip(index + 1) {
            let right_dir = normalized_existing_or_future_path(&right.dir);
            if paths_overlap(&left_dir, &right_dir) {
                diagnostics.push(artifact_diagnostic(
                    &left.dir,
                    format!(
                        "{} `{}` and {} `{}` overlap; choose separate generated output directories",
                        left.label,
                        left.dir.display(),
                        right.label,
                        right.dir.display()
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
        .filter_map(|source| match source.location() {
            SourceLocationSpec::Path(path) => Some(path),
            SourceLocationSpec::Uri(_) => None,
        })
        .map(|path| project.resolve_path(path))
        .collect()
}

fn normalized_existing_or_future_path(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| normalize_path_lexically(path))
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
    left == right || left.starts_with(right) || right.starts_with(left)
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

pub(super) fn artifact_diagnostic_set(path: &Path, message: impl Into<String>) -> DiagnosticSet {
    DiagnosticSet::one(artifact_diagnostic(path, message))
}
