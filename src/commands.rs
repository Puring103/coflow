use crate::artifacts::{CodeOutput, PreparedCodeRelease};
use coflow_codegen_csharp::CsharpCfdCodeGenerator;
use coflow_runtime::codegen::{CodegenInput, CodegenRegistry, CodegenTarget};
use coflow_runtime::Project;
use coflow_runtime::Runtime;
use coflow_runtime::{Diagnostic, DiagnosticSet, Label, Severity, SourceLocation};
use std::path::{Path, PathBuf};

mod id_as_enum;

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

pub fn migrate_enum_lock_after_mutation(
    session: &coflow_runtime::WriteProjectSession,
    report: &coflow_runtime::MutationReport,
) -> Result<bool, DiagnosticSet> {
    id_as_enum::migrate_after_mutation(session.project(), session.queries(), report)
}

#[derive(Debug)]
struct PendingCodegen {
    target_index: usize,
    codegen_id: String,
    display_name: String,
    directory: PathBuf,
    files: Vec<coflow_runtime::codegen::CodeArtifactFile>,
}

pub fn clean_project(project: &Project) -> Result<CleanReport, DiagnosticSet> {
    let (generations_removed, staging_removed) = crate::artifacts::clean_history(project)?;
    Ok(CleanReport {
        generations_removed,
        staging_removed,
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
    prepare_project_code(project, |prepared, _| prepared.has_changes())
}

pub fn generate_project_code(
    project: &Project,
) -> Result<CommandOutcome<CodegenProjectReport>, DiagnosticSet> {
    prepare_project_code(project, |prepared, pending| {
        prepared.publish()?;
        Ok(codegen_report(pending))
    })
}

fn prepare_project_code<T>(
    project: &Project,
    finish: impl FnOnce(PreparedCodeRelease<'_>, &[PendingCodegen]) -> Result<T, DiagnosticSet>,
) -> Result<CommandOutcome<T>, DiagnosticSet> {
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
    let id_as_enum_values =
        id_as_enum::prepare_values(project, session.queries().id_as_enum_info())?;
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
                target: &codegen_target,
                id_as_enum_values: &id_as_enum_values,
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

    let outputs = pending
        .iter()
        .map(|target| CodeOutput {
            slot: format!("codegen:{}", target.target_index),
            directory: target.directory.clone(),
            files: target.files.clone(),
        })
        .collect();
    let prepared = PreparedCodeRelease::new(project, outputs, id_as_enum_values)?;
    finish(prepared, &pending).map(CommandOutcome::Success)
}

fn codegen_report(pending: &[PendingCodegen]) -> CodegenProjectReport {
    let targets = pending
        .iter()
        .map(|target| CodegenReport {
            codegen_id: target.codegen_id.clone(),
            display_name: target.display_name.clone(),
            dir: target.directory.clone(),
        })
        .collect();
    CodegenProjectReport { targets }
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
