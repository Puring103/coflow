mod publication;
mod staging;

pub use publication::{
    enum_lockfile_path, publish_artifacts, read_active_enum_lock, EnumLockUpdate,
    CODE_OUTPUT_SLOT, DATA_OUTPUT_SLOT,
};
pub use staging::StagedArtifactDir;

use coflow_api::{
    ArtifactSet, CodegenContext, Diagnostic, DiagnosticSet, ExportContext, Label, OutputSpec,
    ProviderRegistry, Severity, SourceLocation,
};
use coflow_cft::CompiledSchema;
use coflow_data_model::CfdDataModel;
use coflow_project::{OutputConfig, Project};
use serde_json::Value;
use staging::stage_artifact_set;
use std::path::{Path, PathBuf};

pub fn output_dir(
    project: &Project,
    output: &OutputConfig,
    override_dir: Option<&Path>,
) -> PathBuf {
    override_dir.map_or_else(
        || project.resolve_path(&output.dir),
        |path| project.resolve_path(path),
    )
}

pub fn generate_data_tables(
    registry: &ProviderRegistry,
    schema: &CompiledSchema,
    model: &CfdDataModel,
    exporter_id: &str,
    output_config: &OutputConfig,
    dir: &Path,
) -> Result<ArtifactSet, DiagnosticSet> {
    let exporter = registry.exporter(exporter_id).ok_or_else(|| {
        diagnostic_set(
            dir,
            format!("no data exporter registered for `{exporter_id}`"),
        )
    })?;
    let output = OutputSpec {
        output_type: exporter_id.to_string(),
        dir: dir.to_path_buf(),
        options: output_options(output_config),
    };
    exporter.export(
        ExportContext {
            schema,
            model,
        },
        &output,
    )
}

#[derive(Debug, Clone, Copy)]
pub struct CodegenArtifactRequest<'a> {
    pub schema: &'a CompiledSchema,
    pub model: Option<&'a CfdDataModel>,
    pub codegen_id: &'a str,
    pub data_format: &'a str,
    pub output_config: &'a OutputConfig,
    pub dir: &'a Path,
    pub id_as_enum_variants: &'a Value,
}

pub fn generate_codegen_artifacts(
    registry: &ProviderRegistry,
    request: CodegenArtifactRequest<'_>,
) -> Result<ArtifactSet, DiagnosticSet> {
    let codegen = registry.codegen(request.codegen_id).ok_or_else(|| {
        diagnostic_set(
            request.dir,
            format!("no code generator registered for `{}`", request.codegen_id),
        )
    })?;
    let output = OutputSpec {
        output_type: request.codegen_id.to_string(),
        dir: request.dir.to_path_buf(),
        options: codegen_output_options(request.output_config, request.id_as_enum_variants),
    };
    codegen.generate(
        CodegenContext {
            schema: request.schema,
            model: request.model,
            data_format: request.data_format,
        },
        &output,
    )
}

pub fn stage_artifacts(
    dir: &Path,
    artifacts: ArtifactSet,
) -> Result<StagedArtifactDir, DiagnosticSet> {
    stage_artifact_set(dir, artifacts)
}

pub fn required_data_output<'a>(
    project: &'a Project,
    exporter_id: &str,
    command: &str,
) -> Result<&'a OutputConfig, DiagnosticSet> {
    let output = project.config.outputs.data.as_ref().ok_or_else(|| {
        project_config_diagnostic_set(
            project,
            format!(
                "coflow.yaml missing outputs.data; required `type: {exporter_id}` and `dir` for `{command}`"
            ),
            ["outputs", "data"],
        )
    })?;
    require_output_type(project, output, "data", exporter_id, command)?;
    Ok(output)
}

pub fn required_code_output<'a>(
    project: &'a Project,
    codegen_id: &str,
    command: &str,
) -> Result<&'a OutputConfig, DiagnosticSet> {
    let output = project.config.outputs.code.as_ref().ok_or_else(|| {
        project_config_diagnostic_set(
            project,
            format!(
                "coflow.yaml missing outputs.code; required `type: {codegen_id}` and `dir` for `{command}`"
            ),
            ["outputs", "code"],
        )
    })?;
    require_output_type(project, output, "code", codegen_id, command)?;
    Ok(output)
}

pub fn configured_data_format<'a>(
    project: &'a Project,
    command: &str,
) -> Result<&'a str, DiagnosticSet> {
    let output = project.config.outputs.data.as_ref().ok_or_else(|| {
        project_config_diagnostic_set(
            project,
            format!("coflow.yaml missing outputs.data; required `type` and `dir` for `{command}`"),
            ["outputs", "data"],
        )
    })?;
    Ok(output.output_type.as_str())
}

pub fn configured_data_output<'a>(
    project: &'a Project,
    command: &str,
) -> Result<(&'a OutputConfig, &'a str), DiagnosticSet> {
    let output = project.config.outputs.data.as_ref().ok_or_else(|| {
        project_config_diagnostic_set(
            project,
            format!("coflow.yaml missing outputs.data; required `type` and `dir` for `{command}`"),
            ["outputs", "data"],
        )
    })?;
    Ok((output, output.output_type.as_str()))
}

fn require_output_type(
    project: &Project,
    output: &OutputConfig,
    output_name: &str,
    required_type: &str,
    command: &str,
) -> Result<(), DiagnosticSet> {
    if output.output_type == required_type {
        Ok(())
    } else {
        Err(project_config_diagnostic_set(
            project,
            format!(
            "coflow.yaml outputs.{output_name}.type is `{}`; required `{required_type}` for `{command}`",
            output.output_type
            ),
            ["outputs", output_name, "type"],
        ))
    }
}

fn output_options(output: &OutputConfig) -> Value {
    output.options().clone()
}

fn codegen_output_options(output: &OutputConfig, id_as_enum_variants: &Value) -> Value {
    let mut options = output.options().as_object().cloned().unwrap_or_default();
    if !id_as_enum_variants.is_null() {
        options.insert(
            "id_as_enum_variants".to_string(),
            id_as_enum_variants.clone(),
        );
    }
    Value::Object(options)
}

fn diagnostic_set(path: impl Into<PathBuf>, message: impl Into<String>) -> DiagnosticSet {
    DiagnosticSet::one(Diagnostic {
        code: "ARTIFACT-001".to_string(),
        stage: "ARTIFACT".to_string(),
        severity: Severity::Error,
        message: message.into(),
        primary: Some(Label {
            location: SourceLocation::Artifact { path: path.into() },
            message: None,
        }),
        related: Vec::new(),
    })
}

fn project_config_diagnostic_set(
    project: &Project,
    message: impl Into<String>,
    key_path: impl IntoIterator<Item = impl Into<String>>,
) -> DiagnosticSet {
    DiagnosticSet::one(Diagnostic {
        code: "PROJECT-001".to_string(),
        stage: "PROJECT".to_string(),
        severity: Severity::Error,
        message: message.into(),
        primary: Some(Label {
            location: SourceLocation::ProjectConfig {
                path: project.config_path.clone(),
                key_path: key_path.into_iter().map(Into::into).collect(),
            },
            message: None,
        }),
        related: Vec::new(),
    })
}
