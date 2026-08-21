use coflow_codegen_api::{
    CodegenInput, CodegenRegistry, CodegenTarget, SourceManifestEntry, SourceOrigin,
};
use coflow_codegen_csharp::CsharpCfdCodeGenerator;
use coflow_runtime::{Diagnostic, DiagnosticSet, Label, Severity, SourceLocation};
use coflow_runtime::Project;
use coflow_runtime::Runtime;
use serde_json::Value;
use std::path::{Path, PathBuf};

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

pub fn build_project_status(
    project: &Project,
) -> Result<CommandOutcome<bool>, DiagnosticSet> {
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
    let targets = project.config().codegen.iter().enumerate().collect::<Vec<_>>();
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
    let source_manifest = session
        .queries()
        .source_files()
        .map(|logical_path| SourceManifestEntry {
            logical_path: logical_path.to_string(),
            origin: SourceOrigin::Project,
        })
        .collect::<Vec<_>>();
    let mut generators = CodegenRegistry::default();
    generators.register(CsharpCfdCodeGenerator).map_err(|error| {
        DiagnosticSet::one(project_diagnostic(
            project.config_path(),
            format!("failed to register C# code generator: {error}"),
            ["codegen"],
        ))
    })?;
    let mut reports = Vec::with_capacity(targets.len());
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
        publish_code_files(&directory, artifacts.into_files()).map_err(|message| {
            DiagnosticSet::one(project_diagnostic(
                project.config_path(),
                message,
                vec!["codegen".to_string(), index.to_string(), "dir".to_string()],
            ))
        })?;
        reports.push(CodegenReport {
            codegen_id: target.language.clone(),
            display_name: generator.descriptor().language.to_string(),
            dir: directory,
        });
    }
    if !diagnostics.is_empty() {
        return Ok(CommandOutcome::Diagnostics(diagnostics));
    }
    Ok(CommandOutcome::Success(CodegenProjectReport { targets: reports }))
}

fn publish_code_files(
    directory: &Path,
    files: Vec<coflow_codegen_api::CodeArtifactFile>,
) -> Result<(), String> {
    let parent = directory
        .parent()
        .ok_or_else(|| format!("output directory `{}` has no parent", directory.display()))?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("failed to create output parent: {error}"))?;
    let staging = parent.join(format!(
        ".{}.cfd-staging-{}",
        directory.file_name().and_then(|name| name.to_str()).unwrap_or("generated"),
        std::process::id()
    ));
    if staging.exists() {
        std::fs::remove_dir_all(&staging)
            .map_err(|error| format!("failed to clear staging directory: {error}"))?;
    }
    std::fs::create_dir_all(&staging)
        .map_err(|error| format!("failed to create staging directory: {error}"))?;
    for file in files {
        let target = staging.join(&file.relative_path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("failed to create artifact parent: {error}"))?;
        }
        std::fs::write(&target, file.contents)
            .map_err(|error| format!("failed to write `{}`: {error}", target.display()))?;
    }
    let backup = parent.join(format!(
        ".{}.cfd-backup-{}",
        directory.file_name().and_then(|name| name.to_str()).unwrap_or("generated"),
        std::process::id()
    ));
    if backup.exists() {
        std::fs::remove_dir_all(&backup)
            .map_err(|error| format!("failed to clear artifact backup: {error}"))?;
    }
    if directory.exists() {
        std::fs::rename(directory, &backup)
            .map_err(|error| format!("failed to stage previous output: {error}"))?;
    }
    if let Err(error) = std::fs::rename(&staging, directory) {
        if backup.exists() {
            let _ = std::fs::rename(&backup, directory);
        }
        return Err(format!("failed to publish generated code: {error}"));
    }
    if backup.exists() {
        std::fs::remove_dir_all(&backup)
            .map_err(|error| format!("failed to remove previous output: {error}"))?;
    }
    Ok(())
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
