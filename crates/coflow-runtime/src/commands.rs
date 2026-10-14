use crate::artifacts::{CodeOutput, PreparedCodeRelease};
use crate::codegen::{CodegenInput, CodegenRegistry, CodegenTarget};
use crate::Project;
use crate::Runtime;
use crate::{Diagnostic, DiagnosticSet, Label, Severity, SourceLocation};
use coflow_codegen_csharp::CsharpCfdCodeGenerator;
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

/// Applies a validated mutation and stages related project-file updates.
///
/// # Errors
///
/// Returns diagnostics when related project files cannot be prepared.
pub fn apply_project_mutation(
    session: &mut crate::WriteProjectSession,
    request: crate::MutationRequest,
) -> Result<crate::MutationReport, DiagnosticSet> {
    let project = session.project().clone();
    Ok(
        session.apply_mutation_with_project_files(request, move |queries, applied| {
            id_as_enum::prepare_rename_update(&project, queries, applied)
                .map(|update| update.into_iter().collect())
        }),
    )
}

#[derive(Debug)]
struct PendingCodegen {
    target_index: usize,
    codegen_id: String,
    display_name: String,
    directory: PathBuf,
    files: Vec<crate::codegen::CodeArtifactFile>,
}

/// Checks the project's declarations and selected CFD data.
///
/// # Errors
///
/// Returns diagnostics when the project cannot be opened for checking.
pub fn check_project(project: &Project) -> Result<CommandOutcome<CheckReport>, DiagnosticSet> {
    let mut diagnostics = project.schema_diagnostic_set();
    diagnostics.extend(project.data_diagnostic_set());
    if !diagnostics.is_empty() {
        return Ok(CommandOutcome::Diagnostics(diagnostics));
    }
    let session = Runtime::new().open_data_session(project.clone())?;
    if session.queries().has_diagnostics() {
        Ok(CommandOutcome::Diagnostics(session.into_diagnostics()))
    } else {
        Ok(CommandOutcome::Success(CheckReport))
    }
}

/// Builds all configured generated-code targets.
///
/// # Errors
///
/// Returns diagnostics when code generation or artifact publication fails.
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

/// Reports whether a build would change generated artifacts.
///
/// # Errors
///
/// Returns diagnostics when inputs or existing artifacts cannot be read.
pub fn build_project_status(project: &Project) -> Result<CommandOutcome<bool>, DiagnosticSet> {
    prepare_project_code(project, |prepared, _| prepared.has_changes())
}

/// Generates and publishes all configured code targets.
///
/// # Errors
///
/// Returns diagnostics when generation, staging, or publication fails.
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
    finish: impl FnOnce(PreparedCodeRelease, &[PendingCodegen]) -> Result<T, DiagnosticSet>,
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

    let session = Runtime::new().open_data_session(project.clone())?;
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

/// 为自动化宿主提供只读产物差异和统一发布。
pub fn codegen_automation(
    project: &Project,
    preview: bool,
) -> crate::automation::Result<CommandOutcome<serde_json::Value>> {
    use serde_json::json;
    let snapshot = crate::automation::InputSnapshot::capture(project)?;
    let mut publication_failure = None;
    let outcome = prepare_project_code(project, |prepared, _| {
        let mut report = prepared.preview()?;
        for file in report["files"].as_array_mut().into_iter().flatten() {
            if let Some(path) = file["path"].as_str() {
                file["path"] = json!(crate::project_path(project.root_dir(), Path::new(path)));
            }
        }
        report["written"] = json!(false);
        report["dry_run"] = json!(preview);
        report["written_files"] = json!([]);
        if !preview {
            let failure = match snapshot.verify(project) {
                Err(failure) => Some(failure.with_result(json!({"artifacts":report}))),
                Ok(()) => match prepared.publish_detailed() {
                    Ok(paths) => {
                        report["written"] = json!(!paths.is_empty());
                        report["written_files"] =
                            json!(artifact_report_files(project, &report, &paths));
                        None
                    }
                    Err(failure) => {
                        let failed = if let Some(publication) = failure.publication {
                            let written =
                                artifact_report_files(project, &report, &publication.written);
                            let restored =
                                artifact_report_files(project, &report, &publication.restored);
                            let mut failure = crate::automation::publication_failure(
                                project.root_dir(),
                                publication,
                                report.clone(),
                            );
                            if let Some(report) = failure.result.as_mut() {
                                report["written_files"] = json!(written);
                                report["restored_files"] = json!(restored);
                            }
                            failure.result =
                                failure.result.map(|report| json!({"artifacts":report}));
                            failure
                        } else {
                            let mut failed = crate::automation::Failure::from(failure.diagnostics);
                            failed.status = crate::automation::Status::Error;
                            failed.error_category =
                                Some(crate::automation::ErrorCategory::Execution);
                            failed.with_result(json!({"artifacts":report}))
                        };
                        Some(failed)
                    }
                },
            };
            if let Some(failure) = failure {
                let diagnostics = failure.diagnostics.clone();
                publication_failure = Some(failure);
                return Err(diagnostics);
            }
        }
        Ok(report)
    });
    if let Some(failure) = publication_failure {
        return Err(failure);
    }
    outcome.map_err(crate::automation::Failure::from)
}

fn artifact_report_files(
    project: &Project,
    report: &serde_json::Value,
    paths: &[PathBuf],
) -> Vec<String> {
    report["files"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|file| file["path"].as_str())
        .filter(|file| {
            paths.iter().any(|path| {
                crate::path_is_same_or_descendant(&project.resolve_path(Path::new(file)), path)
            })
        })
        .map(str::to_string)
        .collect()
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
