mod staging;

pub use staging::{
    commit_staged_dir_and_file, commit_staged_dirs_and_file, StagedArtifactDir, StagedArtifactFile,
};

use coflow_api::{
    CfdDataModel, CftContainer, CodegenContext, Diagnostic, DiagnosticSet, ExportContext, Label,
    OutputSpec, ProviderRegistry, Severity, SourceLocation,
};
use coflow_project::{OutputConfig, Project};
use serde::Serialize;
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

pub fn write_data_tables(
    registry: &ProviderRegistry,
    schema: &CftContainer,
    model: &CfdDataModel,
    exporter_id: &str,
    output: &OutputConfig,
    dir: &Path,
) -> Result<(), DiagnosticSet> {
    stage_data_tables(registry, schema, model, exporter_id, output, dir)?.commit()
}

pub fn stage_data_tables(
    registry: &ProviderRegistry,
    schema: &CftContainer,
    model: &CfdDataModel,
    exporter_id: &str,
    output_config: &OutputConfig,
    dir: &Path,
) -> Result<StagedArtifactDir, DiagnosticSet> {
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
    let artifacts = exporter.export(ExportContext { schema, model }, &output)?;
    stage_artifact_set(dir, artifacts)
}

#[derive(Debug, Clone, Copy)]
pub struct CodegenArtifactRequest<'a> {
    pub schema: &'a CftContainer,
    pub model: Option<&'a CfdDataModel>,
    pub codegen_id: &'a str,
    pub data_format: &'a str,
    pub output_config: &'a OutputConfig,
    pub dir: &'a Path,
    pub id_as_enum_variants: &'a Value,
}

pub fn stage_codegen_artifacts(
    registry: &ProviderRegistry,
    request: CodegenArtifactRequest<'_>,
) -> Result<StagedArtifactDir, DiagnosticSet> {
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
    let artifacts = codegen.generate(
        CodegenContext {
            schema: request.schema,
            model: request.model,
            data_format: request.data_format,
        },
        &output,
    )?;
    stage_artifact_set(request.dir, artifacts)
}

pub fn preflight_codegen(
    registry: &ProviderRegistry,
    schema: &CftContainer,
    model: Option<&CfdDataModel>,
    codegen_id: &str,
    data_format: &str,
    output_config: &OutputConfig,
) -> Result<DiagnosticSet, DiagnosticSet> {
    let codegen = registry.codegen(codegen_id).ok_or_else(|| {
        DiagnosticSet::one(Diagnostic::error(
            "CODEGEN",
            "CODEGEN",
            format!("no code generator registered for `{codegen_id}`"),
        ))
    })?;
    let output = OutputSpec {
        output_type: codegen_id.to_string(),
        dir: PathBuf::new(),
        options: codegen_output_options(output_config, &Value::Null),
    };
    Ok(codegen.preflight(
        CodegenContext {
            schema,
            model,
            data_format,
        },
        &output,
    ))
}

pub fn stage_json_file<T: Serialize>(
    path: &Path,
    value: &T,
) -> Result<StagedArtifactFile, DiagnosticSet> {
    StagedArtifactFile::create_json(path, value)
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
