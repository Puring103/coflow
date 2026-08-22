use coflow_codegen_csharp::CsharpCfdCodeGenerator;
use coflow_runtime::codegen::{CodegenInput, CodegenRegistry, CodegenTarget};
use coflow_runtime::Project;
use coflow_runtime::Runtime;
use coflow_runtime::{Diagnostic, DiagnosticSet, Label, Severity, SourceLocation};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug)]
pub enum CommandOutcome<T> {
    Success(T),
    Diagnostics(DiagnosticSet),
}

#[derive(Debug)]
pub struct CheckReport;

#[derive(Debug)]
pub struct CodegenReport {
    pub codegen_id: String,
    pub display_name: String,
    pub dir: PathBuf,
}

#[derive(Debug)]
pub struct CodegenProjectReport {
    pub targets: Vec<CodegenReport>,
}

#[derive(Debug)]
pub struct BuildReport {
    pub targets: Vec<BuildTargetReport>,
}

#[derive(Debug)]
pub struct BuildTargetReport {
    pub target_index: usize,
    pub code: CodegenReport,
}

#[derive(Debug)]
pub struct CleanReport {
    pub generations_removed: usize,
    pub staging_removed: usize,
}

#[derive(Debug)]
struct PendingCodegen {
    target_index: usize,
    codegen_id: String,
    display_name: String,
    directory: PathBuf,
    files: Vec<coflow_runtime::codegen::CodeArtifactFile>,
}

#[derive(Debug)]
struct Publication {
    directory: PathBuf,
    staging: PathBuf,
    backup: PathBuf,
    had_previous: bool,
    published: bool,
}

pub fn clean_project(_project: &Project) -> Result<CleanReport, DiagnosticSet> {
    Ok(CleanReport {
        generations_removed: 0,
        staging_removed: 0,
    })
}

pub fn check_project(project: &Project) -> Result<CommandOutcome<CheckReport>, DiagnosticSet> {
    let mut diagnostics = project.schema_diagnostic_set();
    diagnostics.extend(project.data_diagnostic_set());
    if !diagnostics.is_empty() {
        return Ok(CommandOutcome::Diagnostics(diagnostics));
    }
    let session = Runtime::new().open_read_only_session(project.clone())?;
    if session.queries().has_diagnostics() {
        Ok(CommandOutcome::Diagnostics(session.into_diagnostics()))
    } else {
        Ok(CommandOutcome::Success(CheckReport))
    }
}

pub fn build_project(project: &Project) -> Result<CommandOutcome<BuildReport>, DiagnosticSet> {
    match generate_project_code(project)? {
        CommandOutcome::Success(report) => Ok(CommandOutcome::Success(BuildReport {
            targets: report
                .targets
                .into_iter()
                .enumerate()
                .map(|(target_index, code)| BuildTargetReport { target_index, code })
                .collect(),
        })),
        CommandOutcome::Diagnostics(diagnostics) => Ok(CommandOutcome::Diagnostics(diagnostics)),
    }
}

pub fn build_project_status(project: &Project) -> Result<CommandOutcome<bool>, DiagnosticSet> {
    match generate_project_code(project)? {
        CommandOutcome::Success(_) => Ok(CommandOutcome::Success(false)),
        CommandOutcome::Diagnostics(diagnostics) => Ok(CommandOutcome::Diagnostics(diagnostics)),
    }
}

pub fn generate_project_code(
    project: &Project,
) -> Result<CommandOutcome<CodegenProjectReport>, DiagnosticSet> {
    let mut diagnostics = project.schema_diagnostic_set();
    diagnostics.extend(project.codegen_diagnostic_set());
    let targets = project
        .config()
        .codegen
        .iter()
        .enumerate()
        .collect::<Vec<_>>();
    if targets.is_empty() && diagnostics.is_empty() {
        diagnostics.push(project_diagnostic(
            project.config_path(),
            "coflow.yaml has no codegen target",
            ["codegen"],
        ));
    }
    if !diagnostics.is_empty() {
        return Ok(CommandOutcome::Diagnostics(diagnostics));
    }

    let session = Runtime::new().open_read_only_session(project.clone())?;
    if session.queries().has_diagnostics() {
        return Ok(CommandOutcome::Diagnostics(session.into_diagnostics()));
    }
    let source_manifest = session.queries().codegen_source_manifest();
    let mut generators = CodegenRegistry::default();
    generators
        .register(CsharpCfdCodeGenerator)
        .map_err(|error| {
            DiagnosticSet::one(project_diagnostic(
                project.config_path(),
                format!("failed to register C# code generator: {error}"),
                ["codegen"],
            ))
        })?;
    let mut pending = Vec::with_capacity(targets.len());
    for (index, target) in targets {
        let generator = generators.get(&target.language).ok_or_else(|| {
            DiagnosticSet::one(project_diagnostic(
                project.config_path(),
                format!("unsupported codegen language `{}`", target.language),
                ["codegen", &index.to_string(), "language"],
            ))
        })?;
        let directory = project.resolve_path(&target.dir);
        let codegen_target = CodegenTarget::new(
            target.language.clone(),
            directory.clone(),
            target.options().clone(),
        );
        let artifacts = generator
            .generate(CodegenInput {
                schema: session.schema(),
                model: Some(session.model()),
                sources: &source_manifest,
                target: &codegen_target,
                id_as_enum_lock: &Value::Null,
            })
            .map_err(|error| {
                DiagnosticSet::one(project_diagnostic(
                    project.config_path(),
                    format!("{} code generation failed: {error}", target.language),
                    ["codegen", &index.to_string()],
                ))
            })?;
        pending.push(PendingCodegen {
            target_index: index,
            codegen_id: target.language.clone(),
            display_name: generator.descriptor().language.to_string(),
            directory,
            files: artifacts.into_files(),
        });
    }

    publish_code_batches(&pending).map_err(|(target_index, message)| {
        DiagnosticSet::one(project_diagnostic(
            project.config_path(),
            message,
            vec![
                "codegen".to_string(),
                target_index.to_string(),
                "dir".to_string(),
            ],
        ))
    })?;

    let reports = pending
        .into_iter()
        .map(|target| CodegenReport {
            codegen_id: target.codegen_id,
            display_name: target.display_name,
            dir: target.directory,
        })
        .collect();
    Ok(CommandOutcome::Success(CodegenProjectReport {
        targets: reports,
    }))
}

fn publish_code_batches(pending: &[PendingCodegen]) -> Result<(), (usize, String)> {
    for (position, left) in pending.iter().enumerate() {
        for right in pending.iter().skip(position + 1) {
            if coflow_runtime::path_is_same_or_descendant(&left.directory, &right.directory)
                || coflow_runtime::path_is_same_or_descendant(&right.directory, &left.directory)
            {
                return Err((
                    right.target_index,
                    format!(
                        "codegen output directories `{}` and `{}` overlap",
                        left.directory.display(),
                        right.directory.display()
                    ),
                ));
            }
        }
    }

    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let mut publications = Vec::with_capacity(pending.len());

    for target in pending {
        let parent = target.directory.parent().ok_or_else(|| {
            (
                target.target_index,
                format!(
                    "output directory `{}` has no parent",
                    target.directory.display()
                ),
            )
        })?;
        std::fs::create_dir_all(parent).map_err(|error| {
            (
                target.target_index,
                format!("failed to create output parent: {error}"),
            )
        })?;
        let name = target
            .directory
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("generated");
        let staging = parent.join(format!(
            ".{name}.cfd-staging-{}-{nonce}",
            std::process::id()
        ));
        let backup = parent.join(format!(".{name}.cfd-backup-{}-{nonce}", std::process::id()));
        let publication = Publication {
            directory: target.directory.clone(),
            staging,
            backup,
            had_previous: false,
            published: false,
        };
        if publication.staging.exists() || publication.backup.exists() {
            cleanup_publications(&publications);
            return Err((
                target.target_index,
                format!(
                    "temporary codegen paths for `{}` already exist",
                    target.directory.display()
                ),
            ));
        }
        if let Err(error) = std::fs::create_dir_all(&publication.staging) {
            cleanup_publications(&publications);
            return Err((
                target.target_index,
                format!("failed to create staging directory: {error}"),
            ));
        }
        for file in &target.files {
            let artifact = publication.staging.join(&file.relative_path);
            if let Some(parent) = artifact.parent() {
                if let Err(error) = std::fs::create_dir_all(parent) {
                    cleanup_path(&publication.staging);
                    cleanup_publications(&publications);
                    return Err((
                        target.target_index,
                        format!("failed to create artifact parent: {error}"),
                    ));
                }
            }
            if let Err(error) = std::fs::write(&artifact, &file.contents) {
                cleanup_path(&publication.staging);
                cleanup_publications(&publications);
                return Err((
                    target.target_index,
                    format!("failed to write `{}`: {error}", artifact.display()),
                ));
            }
        }
        publications.push(publication);
    }

    for index in 0..publications.len() {
        let directory = publications[index].directory.clone();
        let backup = publications[index].backup.clone();
        if directory.exists() {
            if let Err(error) = std::fs::rename(&directory, &backup) {
                rollback_publications(&mut publications);
                return Err((
                    pending[index].target_index,
                    format!("failed to stage previous output: {error}"),
                ));
            }
            publications[index].had_previous = true;
        }
    }

    for index in 0..publications.len() {
        let staging = publications[index].staging.clone();
        let directory = publications[index].directory.clone();
        if let Err(error) = std::fs::rename(&staging, &directory) {
            rollback_publications(&mut publications);
            return Err((
                pending[index].target_index,
                format!("failed to publish generated code: {error}"),
            ));
        }
        publications[index].published = true;
    }

    for publication in &publications {
        cleanup_path(&publication.backup);
    }
    Ok(())
}

fn rollback_publications(publications: &mut [Publication]) {
    for publication in publications.iter().rev() {
        if publication.published {
            cleanup_path(&publication.directory);
        }
    }
    for publication in publications.iter().rev() {
        if publication.had_previous && publication.backup.exists() {
            let _ = std::fs::rename(&publication.backup, &publication.directory);
        }
    }
    cleanup_publications(publications);
}

fn cleanup_publications(publications: &[Publication]) {
    for publication in publications {
        cleanup_path(&publication.staging);
        cleanup_path(&publication.backup);
    }
}

fn cleanup_path(path: &Path) {
    if path.is_dir() {
        let _ = std::fs::remove_dir_all(path);
    } else if path.exists() {
        let _ = std::fs::remove_file(path);
    }
}

fn project_diagnostic(
    config_path: &Path,
    message: impl Into<String>,
    key_path: impl IntoIterator<Item = impl Into<String>>,
) -> Diagnostic {
    Diagnostic {
        code: "PROJECT-001".to_string(),
        stage: "PROJECT".to_string(),
        severity: Severity::Error,
        message: message.into(),
        primary: Some(Label {
            location: SourceLocation::ProjectConfig {
                path: config_path.to_path_buf(),
                key_path: key_path.into_iter().map(Into::into).collect(),
            },
            message: None,
        }),
        related: Vec::new(),
        contexts: Vec::new(),
    }
}
