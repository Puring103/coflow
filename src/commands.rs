use crate::artifacts::{
    configured_data_format, configured_data_output, required_code_output, required_data_output,
    ArtifactReleasePlan, CODE_OUTPUT_SLOT, DATA_OUTPUT_SLOT,
};
use coflow_api::{Diagnostic, DiagnosticSet, Label, ProviderRegistry, Severity, SourceLocation};
use coflow_project::Project;
use coflow_runtime::Runtime;
use id_as_enum::{id_as_enum_variants_for_schema_only, prepare_id_as_enum_artifacts_for_build};
use serde_json::Value;
use std::path::{Path, PathBuf};

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

#[derive(Debug)]
pub struct CleanReport {
    pub generations_removed: usize,
    pub staging_removed: usize,
}

/// Removes inactive artifact generations and abandoned staging entries.
///
/// # Errors
///
/// Returns diagnostics when artifact state cannot be read or removed.
pub fn clean_project(project: &Project) -> Result<CleanReport, DiagnosticSet> {
    let (generations_removed, staging_removed) = crate::artifacts::clean_history(project)?;
    Ok(CleanReport {
        generations_removed,
        staging_removed,
    })
}

/// Runs schema, data loading, and CFT check validation for a project.
///
/// # Errors
///
/// Returns an error for unrecoverable project/schema I/O errors. User-fixable
/// project, schema, data loading, data-model, and check diagnostics are
/// returned as `CommandOutcome::Diagnostics`.
pub fn check_project(
    project: &Project,
    registry: &ProviderRegistry,
) -> Result<CommandOutcome<CheckReport>, DiagnosticSet> {
    let mut diagnostics = project.schema_diagnostic_set();
    diagnostics.extend(project.data_diagnostic_set());
    if !diagnostics.is_empty() {
        return Ok(CommandOutcome::Diagnostics(diagnostics));
    }
    let runtime = Runtime::new(registry.clone());
    let session = runtime.open_read_only_session(project.clone())?;
    if session.queries().has_diagnostics() {
        Ok(CommandOutcome::Diagnostics(session.into_diagnostics()))
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
    project: &Project,
    registry: &ProviderRegistry,
    options: BuildOptions<'_>,
) -> Result<CommandOutcome<BuildReport>, DiagnosticSet> {
    let diagnostics = build_config_diagnostics(project);
    if !diagnostics.is_empty() {
        return Ok(CommandOutcome::Diagnostics(diagnostics));
    }
    let runtime = Runtime::new(registry.clone());
    let session = runtime.build_project_session(project.clone())?;
    if session.queries().has_diagnostics() {
        return Ok(CommandOutcome::Diagnostics(session.into_diagnostics()));
    }
    let (data_output, data_format) = configured_data_output(project, "coflow build")?;
    let Some(exporter) = registry.exporter(data_format) else {
        return Ok(CommandOutcome::Diagnostics(project_diagnostic_set(
            &project.config_path,
            format!("no data exporter registered for `{data_format}`"),
            ["outputs", "data", "type"],
        )));
    };
    let code = if let Some(output) = project.config.outputs.code.as_ref() {
        let Some(codegen) = registry.codegen(&output.output_type) else {
            return Ok(CommandOutcome::Diagnostics(project_diagnostic_set(
                &project.config_path,
                format!("no code generator registered for `{}`", output.output_type),
                ["outputs", "code", "type"],
            )));
        };
        Some((output, codegen))
    } else {
        None
    };
    let (id_as_enum_variants, enum_lock_state) = if code.is_some() {
        match prepare_id_as_enum_artifacts_for_build(project, session.queries().id_as_enum_info()) {
            Ok(artifacts) => (artifacts.variants, Some(artifacts.lock_state)),
            Err(diagnostics) => return Ok(CommandOutcome::Diagnostics(diagnostics)),
        }
    } else {
        (Value::Null, None)
    };
    let mut release = ArtifactReleasePlan::new(project);
    release.add_data(&session, exporter, data_output, options.data_out_dir);
    let has_code = code.is_some();
    if let Some((output, codegen)) = code {
        release.add_build_code(
            &session,
            codegen,
            output,
            options.code_out_dir,
            data_format,
            &id_as_enum_variants,
        );
        if let Some(lock_state) = enum_lock_state {
            release.replace_enum_lock(lock_state);
        }
    } else {
        release.remove_output(CODE_OUTPUT_SLOT);
    }
    let published = match release.execute() {
        Ok(published) => published,
        Err(diagnostics) => return Ok(CommandOutcome::Diagnostics(diagnostics)),
    };
    let data = export_report(published.output(DATA_OUTPUT_SLOT)?);
    let code = has_code
        .then(|| published.output(CODE_OUTPUT_SLOT).map(codegen_report))
        .transpose()?;
    Ok(CommandOutcome::Success(BuildReport { data, code }))
}

/// Exports project data in the requested format.
///
/// # Errors
///
/// Returns an error for unrecoverable project/schema I/O errors. User-fixable
/// diagnostics are returned as `CommandOutcome::Diagnostics`.
pub fn export_project_data(
    project: &Project,
    registry: &ProviderRegistry,
    exporter_id: &str,
    options: ExportOptions<'_>,
) -> Result<CommandOutcome<ExportReport>, DiagnosticSet> {
    let mut diagnostics = project.schema_diagnostic_set();
    diagnostics.extend(project.data_diagnostic_set());
    let command = format!("coflow export {exporter_id}");
    if let Err(output_diagnostics) = required_data_output(project, exporter_id, &command) {
        diagnostics.extend(output_diagnostics);
    }
    if !diagnostics.is_empty() {
        return Ok(CommandOutcome::Diagnostics(diagnostics));
    }
    let output = required_data_output(project, exporter_id, &command)?;
    let runtime = Runtime::new(registry.clone());
    let session = runtime.build_project_session(project.clone())?;
    if session.queries().has_diagnostics() {
        return Ok(CommandOutcome::Diagnostics(session.into_diagnostics()));
    }
    let Some(exporter) = registry.exporter(exporter_id) else {
        return Ok(CommandOutcome::Diagnostics(project_diagnostic_set(
            &project.config_path,
            format!("no data exporter registered for `{exporter_id}`"),
            ["outputs", "data", "type"],
        )));
    };
    let mut release = ArtifactReleasePlan::new(project);
    release.add_data(&session, exporter, output, options.out_dir);
    let published = match release.execute() {
        Ok(published) => published,
        Err(diagnostics) => return Ok(CommandOutcome::Diagnostics(diagnostics)),
    };
    Ok(CommandOutcome::Success(export_report(
        published.output(DATA_OUTPUT_SLOT)?,
    )))
}

/// Generates project code for the requested target.
///
/// # Errors
///
/// Returns an error for invalid codegen configuration, unsupported target/data
/// format combinations, or code artifact write failures. Schema diagnostics are
/// returned as `CommandOutcome::Diagnostics`.
pub fn generate_project_code(
    project: &Project,
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
    let output = required_code_output(project, codegen_id, &command)?;
    let data_format = configured_data_format(project, &command)?;
    let session = Runtime::open_schema_session(project.clone())?;
    if session.has_diagnostics() {
        return Ok(CommandOutcome::Diagnostics(session.into_diagnostics()));
    }
    let Some(codegen) = registry.codegen(codegen_id) else {
        return Ok(CommandOutcome::Diagnostics(project_diagnostic_set(
            &project.config_path,
            format!("no code generator registered for `{codegen_id}`"),
            ["outputs", "code", "type"],
        )));
    };
    let variants = match id_as_enum_variants_for_schema_only(project) {
        Ok(variants) => variants,
        Err(diagnostics) => return Ok(CommandOutcome::Diagnostics(diagnostics)),
    };
    let mut release = ArtifactReleasePlan::new(project);
    release.add_schema_code(
        &session,
        codegen,
        output,
        options.out_dir,
        data_format,
        &variants,
    );
    let published = match release.execute() {
        Ok(published) => published,
        Err(diagnostics) => return Ok(CommandOutcome::Diagnostics(diagnostics)),
    };
    Ok(CommandOutcome::Success(codegen_report(
        published.output(CODE_OUTPUT_SLOT)?,
    )))
}

fn build_config_diagnostics(project: &Project) -> DiagnosticSet {
    let mut diagnostics = project.schema_diagnostic_set();
    diagnostics.extend(project.data_diagnostic_set());
    if let Err(output_diagnostics) = configured_data_output(project, "coflow build") {
        diagnostics.extend(output_diagnostics);
    }
    diagnostics
}

fn export_report(output: &crate::artifacts::ReleasedOutput) -> ExportReport {
    ExportReport {
        exporter_id: output.provider_id.clone(),
        display_name: output.display_name.to_string(),
        dir: output.dir.clone(),
    }
}

fn codegen_report(output: &crate::artifacts::ReleasedOutput) -> CodegenReport {
    CodegenReport {
        codegen_id: output.provider_id.clone(),
        display_name: output.display_name.to_string(),
        dir: output.dir.clone(),
    }
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
