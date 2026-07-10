use crate::artifacts::{
    commit_staged_dir_and_file, commit_staged_dirs_and_file, configured_data_format,
    configured_data_output, output_dir, preflight_codegen, required_code_output,
    required_data_output, stage_codegen_artifacts, stage_data_tables, write_data_tables,
    CodegenArtifactRequest,
};
use artifact_safety::{artifact_safety_diagnostics, ArtifactOutputPlan};
use coflow_api::{Diagnostic, DiagnosticSet, Label, ProviderRegistry, Severity, SourceLocation};
use coflow_project::{OutputConfig, Project};
use coflow_runtime::{ProjectSession, Runtime};
use id_as_enum::{id_as_enum_variants_for_schema_only, stage_id_as_enum_lockfile_for_build};
use std::path::{Path, PathBuf};

mod artifact_safety;
mod id_as_enum;

pub const JSON_EXPORTER_ID: &str = "json";
pub const MESSAGEPACK_EXPORTER_ID: &str = "messagepack";
pub const CSHARP_CODEGEN_ID: &str = "csharp";

#[derive(Debug)]
pub enum CommandOutcome<T> {
    Success(T),
    Diagnostics(DiagnosticSet),
}

#[derive(Debug, Clone, Copy, Default)]
pub struct BuildOptions<'a> {
    pub data_out_dir: Option<&'a Path>,
    pub code_out_dir: Option<&'a Path>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ExportOptions<'a> {
    pub out_dir: Option<&'a Path>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct CodegenOptions<'a> {
    pub out_dir: Option<&'a Path>,
}

#[derive(Debug)]
pub struct CheckReport;

#[derive(Debug)]
pub struct ExportReport {
    pub exporter_id: String,
    pub display_name: String,
    pub dir: PathBuf,
}

#[derive(Debug)]
pub struct CodegenReport {
    pub codegen_id: String,
    pub display_name: String,
    pub dir: PathBuf,
}

#[derive(Debug)]
pub struct BuildReport {
    pub data: ExportReport,
    pub code: Option<CodegenReport>,
}

/// Runs schema, data loading, and CFT check validation for a project.
///
/// # Errors
///
/// Returns an error for unrecoverable project/schema I/O errors. User-fixable
/// project, schema, data loading, data-model, and check diagnostics are
/// returned as `CommandOutcome::Diagnostics`.
pub fn check_project(
    project: Project,
    registry: &ProviderRegistry,
) -> Result<CommandOutcome<CheckReport>, DiagnosticSet> {
    let runtime = Runtime::new(registry.clone());
    let session = runtime.build_project_session(project)?.into_session();
    if session.has_diagnostics() {
        Ok(CommandOutcome::Diagnostics(session.diagnostics.into_set()))
    } else {
        Ok(CommandOutcome::Success(CheckReport))
    }
}

/// Runs validation, data export, and configured code generation.
///
/// # Errors
///
/// Returns an error for unrecoverable project/schema I/O errors. User-fixable
/// diagnostics are returned as `CommandOutcome::Diagnostics`.
pub fn build_project(
    project: Project,
    registry: &ProviderRegistry,
    options: BuildOptions<'_>,
) -> Result<CommandOutcome<BuildReport>, DiagnosticSet> {
    let diagnostics = build_config_diagnostics(&project);
    if !diagnostics.is_empty() {
        return Ok(CommandOutcome::Diagnostics(diagnostics));
    }
    let plan = match build_provider_plan(&project, registry, options) {
        Ok(plan) => plan,
        Err(diagnostics) => return Ok(CommandOutcome::Diagnostics(diagnostics)),
    };
    let runtime = Runtime::new(registry.clone());
    let session = runtime.build_project_session(project)?.into_session();
    if session.has_diagnostics() {
        return Ok(CommandOutcome::Diagnostics(session.diagnostics.into_set()));
    }

    let mut preflight_diagnostics = build_codegen_preflight_diagnostics(registry, &session, &plan)?;
    preflight_diagnostics.extend(artifact_safety_diagnostics(
        &session.project,
        &plan.artifact_outputs,
    ));
    if !preflight_diagnostics.is_empty() {
        return Ok(CommandOutcome::Diagnostics(preflight_diagnostics));
    }

    let staged_data = match stage_data_tables(
        registry,
        &session.schema,
        &session.model,
        &plan.data.exporter_id,
        &plan.data.output,
        &plan.data.dir,
    ) {
        Ok(staged_data) => staged_data,
        Err(diagnostics) => return Ok(CommandOutcome::Diagnostics(diagnostics)),
    };
    let code = match commit_build_artifacts(&session, registry, staged_data, &plan) {
        Ok(code) => code,
        Err(diagnostics) => return Ok(CommandOutcome::Diagnostics(diagnostics)),
    };

    let data = ExportReport {
        exporter_id: plan.data.exporter_id.clone(),
        display_name: plan.data.display_name.to_string(),
        dir: plan.data.dir,
    };

    Ok(CommandOutcome::Success(BuildReport { data, code }))
}

/// Exports project data in the requested format.
///
/// # Errors
///
/// Returns an error for unrecoverable project/schema I/O errors. User-fixable
/// diagnostics are returned as `CommandOutcome::Diagnostics`.
pub fn export_project_data(
    project: Project,
    registry: &ProviderRegistry,
    exporter_id: &str,
    options: ExportOptions<'_>,
) -> Result<CommandOutcome<ExportReport>, DiagnosticSet> {
    let mut diagnostics = project.schema_diagnostic_set();
    diagnostics.extend(project.data_diagnostic_set());
    let command = format!("coflow export {exporter_id}");
    if let Err(output_diagnostics) = required_data_output(&project, exporter_id, &command) {
        diagnostics.extend(output_diagnostics);
    }
    if !diagnostics.is_empty() {
        return Ok(CommandOutcome::Diagnostics(diagnostics));
    }
    let Some(exporter) = registry.exporter(exporter_id) else {
        return Ok(CommandOutcome::Diagnostics(project_diagnostic_set(
            &project.config_path,
            format!("no data exporter registered for `{exporter_id}`"),
            ["outputs", "data", "type"],
        )));
    };
    let exporter_descriptor = exporter.descriptor();
    let output = required_data_output(&project, exporter_id, &command)?.clone();
    let dir = output_dir(&project, &output, options.out_dir);
    let runtime = Runtime::new(registry.clone());
    let session = runtime.build_project_session(project)?.into_session();
    if session.has_diagnostics() {
        return Ok(CommandOutcome::Diagnostics(session.diagnostics.into_set()));
    }
    let artifact_diagnostics = artifact_safety_diagnostics(
        &session.project,
        &[ArtifactOutputPlan::new("outputs.data.dir", dir.clone())],
    );
    if !artifact_diagnostics.is_empty() {
        return Ok(CommandOutcome::Diagnostics(artifact_diagnostics));
    }
    if let Err(diagnostics) = write_data_tables(
        registry,
        &session.schema,
        &session.model,
        exporter_id,
        &output,
        &dir,
    ) {
        return Ok(CommandOutcome::Diagnostics(diagnostics));
    }
    Ok(CommandOutcome::Success(ExportReport {
        exporter_id: exporter_id.to_string(),
        display_name: exporter_descriptor.display_name.to_string(),
        dir,
    }))
}

/// Generates project code for the requested target.
///
/// # Errors
///
/// Returns an error for invalid codegen configuration, unsupported target/data
/// format combinations, or code artifact write failures. Schema diagnostics are
/// returned as `CommandOutcome::Diagnostics`.
pub fn generate_project_code(
    project: Project,
    registry: &ProviderRegistry,
    codegen_id: &str,
    options: CodegenOptions<'_>,
) -> Result<CommandOutcome<CodegenReport>, DiagnosticSet> {
    let mut diagnostics = project.schema_diagnostic_set();
    diagnostics.extend(project.codegen_diagnostic_set());
    if !diagnostics.is_empty() {
        return Ok(CommandOutcome::Diagnostics(diagnostics));
    }
    let command = format!("coflow codegen {codegen_id}");
    let output = required_code_output(&project, codegen_id, &command)?.clone();
    let data_format = configured_data_format(&project, &command)?.to_string();
    let Some(codegen) = registry.codegen(codegen_id) else {
        return Ok(CommandOutcome::Diagnostics(project_diagnostic_set(
            &project.config_path,
            format!("no code generator registered for `{codegen_id}`"),
            ["outputs", "code", "type"],
        )));
    };
    let codegen_descriptor = codegen.descriptor();
    if !codegen_descriptor
        .supported_data_formats
        .contains(&data_format.as_str())
    {
        return Ok(CommandOutcome::Diagnostics(project_diagnostic_set(
            &project.config_path,
            format!("code generator `{codegen_id}` does not support data format `{data_format}`"),
            ["outputs", "code", "type"],
        )));
    }
    let dir = output_dir(&project, &output, options.out_dir);
    let session = Runtime::build_schema_session(project)?;
    if session.has_diagnostics() {
        return Ok(CommandOutcome::Diagnostics(session.diagnostics.into_set()));
    }
    let codegen_diagnostics = preflight_codegen(
        registry,
        &session.schema,
        None,
        codegen_id,
        &data_format,
        &output,
    )?;
    if !codegen_diagnostics.is_empty() {
        return Ok(CommandOutcome::Diagnostics(codegen_diagnostics));
    }
    let artifact_diagnostics = artifact_safety_diagnostics(
        &session.project,
        &[ArtifactOutputPlan::new("outputs.code.dir", dir.clone())],
    );
    if !artifact_diagnostics.is_empty() {
        return Ok(CommandOutcome::Diagnostics(artifact_diagnostics));
    }
    let id_as_enum_variants = match id_as_enum_variants_for_schema_only(&session.project) {
        Ok(variants) => variants,
        Err(diagnostics) => return Ok(CommandOutcome::Diagnostics(diagnostics)),
    };
    let staged_code = match stage_codegen_artifacts(
        registry,
        CodegenArtifactRequest {
            schema: &session.schema,
            model: None,
            codegen_id,
            data_format: &data_format,
            output_config: &output,
            dir: &dir,
            id_as_enum_variants: &id_as_enum_variants,
        },
    ) {
        Ok(staged_code) => staged_code,
        Err(diagnostics) => return Ok(CommandOutcome::Diagnostics(diagnostics)),
    };
    if let Err(diagnostics) = commit_staged_dir_and_file(staged_code, None) {
        return Ok(CommandOutcome::Diagnostics(diagnostics));
    }
    Ok(CommandOutcome::Success(CodegenReport {
        codegen_id: codegen_id.to_string(),
        display_name: codegen_descriptor.display_name.to_string(),
        dir,
    }))
}

#[derive(Debug)]
struct BuildProviderPlan {
    data: BuildDataPlan,
    code: Option<BuildCodegenPlan>,
    artifact_outputs: Vec<ArtifactOutputPlan>,
}

#[derive(Debug)]
struct BuildDataPlan {
    output: OutputConfig,
    exporter_id: String,
    display_name: &'static str,
    dir: PathBuf,
}

#[derive(Debug)]
struct BuildCodegenPlan {
    output: OutputConfig,
    codegen_id: String,
    display_name: &'static str,
    dir: PathBuf,
    needs_model_for_build: bool,
}

fn build_config_diagnostics(project: &Project) -> DiagnosticSet {
    let mut diagnostics = project.schema_diagnostic_set();
    diagnostics.extend(project.data_diagnostic_set());
    if let Err(output_diagnostics) = configured_data_output(project, "coflow build") {
        diagnostics.extend(output_diagnostics);
    }
    diagnostics
}

fn build_provider_plan<'a>(
    project: &'a Project,
    registry: &ProviderRegistry,
    options: BuildOptions<'a>,
) -> Result<BuildProviderPlan, DiagnosticSet> {
    let (data_output, data_format) = configured_data_output(project, "coflow build")?;
    let data_exporter = registry.exporter(data_format).ok_or_else(|| {
        project_diagnostic_set(
            &project.config_path,
            format!("no data exporter registered for `{data_format}`"),
            ["outputs", "data", "type"],
        )
    })?;
    let data_dir = output_dir(project, data_output, options.data_out_dir);
    let mut artifact_outputs = vec![ArtifactOutputPlan::new(
        "outputs.data.dir",
        data_dir.clone(),
    )];
    let code = build_codegen_plan(
        project,
        registry,
        options,
        data_format,
        &mut artifact_outputs,
    )?;

    Ok(BuildProviderPlan {
        data: BuildDataPlan {
            output: data_output.clone(),
            exporter_id: data_format.to_string(),
            display_name: data_exporter.descriptor().display_name,
            dir: data_dir,
        },
        code,
        artifact_outputs,
    })
}

fn build_codegen_plan<'a>(
    project: &'a Project,
    registry: &ProviderRegistry,
    options: BuildOptions<'a>,
    data_format: &str,
    artifact_outputs: &mut Vec<ArtifactOutputPlan>,
) -> Result<Option<BuildCodegenPlan>, DiagnosticSet> {
    let Some(output) = project.config.outputs.code.as_ref() else {
        return Ok(None);
    };
    let codegen_id = output.output_type.clone();
    let codegen = registry.codegen(&codegen_id).ok_or_else(|| {
        project_diagnostic_set(
            &project.config_path,
            format!("no code generator registered for `{codegen_id}`"),
            ["outputs", "code", "type"],
        )
    })?;
    let descriptor = codegen.descriptor();
    if !descriptor.supported_data_formats.contains(&data_format) {
        return Err(project_diagnostic_set(
            &project.config_path,
            format!("code generator `{codegen_id}` does not support data format `{data_format}`"),
            ["outputs", "code", "type"],
        ));
    }

    let dir = output_dir(project, output, options.code_out_dir);
    artifact_outputs.push(ArtifactOutputPlan::new("outputs.code.dir", dir.clone()));
    Ok(Some(BuildCodegenPlan {
        output: output.clone(),
        codegen_id,
        display_name: descriptor.display_name,
        dir,
        needs_model_for_build: descriptor.needs_model_for_build,
    }))
}

fn build_codegen_preflight_diagnostics(
    registry: &ProviderRegistry,
    session: &ProjectSession,
    plan: &BuildProviderPlan,
) -> Result<DiagnosticSet, DiagnosticSet> {
    let Some(code) = plan.code.as_ref() else {
        return Ok(DiagnosticSet::empty());
    };
    preflight_codegen(
        registry,
        &session.schema,
        code.needs_model_for_build.then_some(&session.model),
        &code.codegen_id,
        &plan.data.exporter_id,
        &code.output,
    )
}

fn commit_build_artifacts(
    session: &ProjectSession,
    registry: &ProviderRegistry,
    staged_data: crate::artifacts::StagedArtifactDir,
    plan: &BuildProviderPlan,
) -> Result<Option<CodegenReport>, DiagnosticSet> {
    let Some(code) = plan.code.as_ref() else {
        staged_data.commit()?;
        return Ok(None);
    };

    let id_as_enum_artifacts =
        stage_id_as_enum_lockfile_for_build(&session.project, &session.schema, &session.model)?;
    let staged_code = stage_codegen_artifacts(
        registry,
        CodegenArtifactRequest {
            schema: &session.schema,
            model: code.needs_model_for_build.then_some(&session.model),
            codegen_id: &code.codegen_id,
            data_format: &plan.data.exporter_id,
            output_config: &code.output,
            dir: &code.dir,
            id_as_enum_variants: &id_as_enum_artifacts.variants,
        },
    )?;
    commit_staged_dirs_and_file(
        vec![staged_data, staged_code],
        id_as_enum_artifacts.lockfile,
    )?;
    Ok(Some(CodegenReport {
        codegen_id: code.codegen_id.clone(),
        display_name: code.display_name.to_string(),
        dir: code.dir.clone(),
    }))
}

fn project_diagnostic_set(
    config_path: &Path,
    message: impl Into<String>,
    key_path: impl IntoIterator<Item = impl Into<String>>,
) -> DiagnosticSet {
    DiagnosticSet::one(project_diagnostic(config_path, message, key_path))
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
    }
}
