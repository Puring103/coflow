use crate::artifacts::{
    code_output_slot, data_output_slot, required_code_output, required_data_output,
    ArtifactReleasePlan,
};
use coflow_api::{Diagnostic, DiagnosticSet, Label, ProviderRegistry, Severity, SourceLocation};
use coflow_project::Project;
use coflow_runtime::Runtime;
use id_as_enum::{id_as_enum_variants_for_schema_only, prepare_id_as_enum_artifacts_for_build};
use serde_json::Value;
use std::collections::BTreeSet;
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
    pub targets: Vec<BuildTargetReport>,
}

#[derive(Debug)]
pub struct BuildTargetReport {
    pub target_index: usize,
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
    let mut diagnostics = DiagnosticSet::empty();
    let mut targets = Vec::new();
    for (index, target) in project.config.outputs.targets().iter().enumerate() {
        let exporter = registry.exporter(&target.data.output_type);
        if exporter.is_none() {
            diagnostics.push(output_target_diagnostic(
                project,
                index,
                "data",
                format!(
                    "no data exporter registered for `{}`",
                    target.data.output_type
                ),
            ));
        }
        let code = target.code.as_ref().and_then(|output| {
            let codegen = registry.codegen(&output.output_type);
            if codegen.is_none() {
                diagnostics.push(output_target_diagnostic(
                    project,
                    index,
                    "code",
                    format!("no code generator registered for `{}`", output.output_type),
                ));
            }
            let explicit_loader = target
                .loader
                .as_ref()
                .map(|loader| loader.loader_type.as_str());
            let loader = registry.select_loader(
                &output.output_type,
                &target.data.output_type,
                explicit_loader,
            );
            if loader.is_none() {
                let message = explicit_loader.map_or_else(
                    || {
                        format!(
                            "no loader registered for code `{}` and data `{}`",
                            output.output_type, target.data.output_type
                        )
                    },
                    |loader| {
                        format!(
                            "loader `{loader}` is not registered for code `{}` and data `{}`",
                            output.output_type, target.data.output_type
                        )
                    },
                );
                diagnostics.push(output_target_diagnostic(project, index, "loader", message));
            }
            codegen.zip(loader)
        });
        if let Some(exporter) = exporter {
            targets.push((index, target, exporter, code));
        }
    }
    if !diagnostics.is_empty() {
        return Ok(CommandOutcome::Diagnostics(diagnostics));
    }
    let has_code = targets
        .iter()
        .any(|(_, target, _, _)| target.code.is_some());
    let (id_as_enum_variants, enum_lock_state) = if has_code {
        match prepare_id_as_enum_artifacts_for_build(project, session.queries().id_as_enum_info()) {
            Ok(artifacts) => (artifacts.variants, Some(artifacts.lock_state)),
            Err(diagnostics) => return Ok(CommandOutcome::Diagnostics(diagnostics)),
        }
    } else {
        (Value::Null, None)
    };
    let mut release = ArtifactReleasePlan::new(project);
    let mut planned_slots = BTreeSet::new();
    for (index, target, exporter, code) in targets {
        let data_slot = data_output_slot(index);
        let data_override = (index == 0).then_some(options.data_out_dir).flatten();
        release.add_data_for_slot(
            data_slot.clone(),
            &session,
            exporter.clone(),
            &target.data,
            data_override,
        );
        planned_slots.insert(data_slot);
        if let (Some(output), Some((codegen, loader))) = (&target.code, code) {
            let code_slot = code_output_slot(index);
            let code_override = (index == 0).then_some(options.code_out_dir).flatten();
            release.add_build_code_with_loader_for_slot(
                code_slot.clone(),
                &session,
                codegen,
                loader,
                exporter,
                output,
                &target.data,
                target.loader_options(),
                code_override,
                &id_as_enum_variants,
            );
            planned_slots.insert(code_slot);
        }
    }
    release.remove_stale_managed_outputs(&planned_slots)?;
    if let Some(lock_state) = enum_lock_state {
        release.replace_enum_lock(lock_state);
    }
    let published = match release.execute() {
        Ok(published) => published,
        Err(diagnostics) => return Ok(CommandOutcome::Diagnostics(diagnostics)),
    };
    let mut reports = Vec::with_capacity(project.config.outputs.targets().len());
    for (index, target) in project.config.outputs.targets().iter().enumerate() {
        let data = export_report(published.output(&data_output_slot(index))?);
        let code = target
            .code
            .as_ref()
            .map(|_| {
                published
                    .output(&code_output_slot(index))
                    .map(codegen_report)
            })
            .transpose()?;
        reports.push(BuildTargetReport {
            target_index: index,
            data,
            code,
        });
    }
    Ok(CommandOutcome::Success(BuildReport { targets: reports }))
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
    let (target_index, output) = required_data_output(project, exporter_id, &command)?;
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
    let slot = data_output_slot(target_index);
    release.add_data_for_slot(&slot, &session, exporter, output, options.out_dir);
    let published = match release.execute() {
        Ok(published) => published,
        Err(diagnostics) => return Ok(CommandOutcome::Diagnostics(diagnostics)),
    };
    Ok(CommandOutcome::Success(export_report(
        published.output(&slot)?,
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
    let (target_index, target, output) = required_code_output(project, codegen_id, &command)?;
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
    let explicit_loader = target
        .loader
        .as_ref()
        .map(|loader| loader.loader_type.as_str());
    let Some(loader) =
        registry.select_loader(codegen_id, &target.data.output_type, explicit_loader)
    else {
        return Ok(CommandOutcome::Diagnostics(project_diagnostic_set(
            &project.config_path,
            format!(
                "code generator `{codegen_id}` does not support data format `{}`: no compatible loader is registered",
                target.data.output_type
            ),
            ["outputs", "loader", "type"],
        )));
    };
    let Some(exporter) = registry.exporter(&target.data.output_type) else {
        return Ok(CommandOutcome::Diagnostics(project_diagnostic_set(
            &project.config_path,
            format!(
                "no data exporter registered for `{}`",
                target.data.output_type
            ),
            ["outputs", "data", "type"],
        )));
    };
    let variants = match id_as_enum_variants_for_schema_only(project) {
        Ok(variants) => variants,
        Err(diagnostics) => return Ok(CommandOutcome::Diagnostics(diagnostics)),
    };
    let mut release = ArtifactReleasePlan::new(project);
    let slot = code_output_slot(target_index);
    release.add_schema_code_with_loader_for_slot(
        &slot,
        &session,
        codegen,
        loader,
        exporter,
        output,
        &target.data,
        target.loader_options(),
        options.out_dir,
        &variants,
    );
    let published = match release.execute() {
        Ok(published) => published,
        Err(diagnostics) => return Ok(CommandOutcome::Diagnostics(diagnostics)),
    };
    Ok(CommandOutcome::Success(codegen_report(
        published.output(&slot)?,
    )))
}

fn build_config_diagnostics(project: &Project) -> DiagnosticSet {
    let mut diagnostics = project.schema_diagnostic_set();
    diagnostics.extend(project.data_diagnostic_set());
    if project.config.outputs.targets().is_empty() {
        diagnostics.push(project_diagnostic(
            &project.config_path,
            "coflow.yaml missing outputs.data",
            ["outputs", "data"],
        ));
    }
    diagnostics
}

fn output_target_diagnostic(
    project: &Project,
    target_index: usize,
    component: &str,
    message: impl Into<String>,
) -> Diagnostic {
    let mut key_path = vec!["outputs".to_string()];
    if !project.config.outputs.is_object_shape() {
        key_path.push(target_index.to_string());
    }
    key_path.push(component.to_string());
    key_path.push("type".to_string());
    project_diagnostic(&project.config_path, message, key_path)
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
