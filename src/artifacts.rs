mod fault;
mod publication;
mod staging;

pub use publication::{
    enum_lockfile_path, publish_artifacts, read_active_enum_lock, EnumLockUpdate, CODE_OUTPUT_SLOT,
    DATA_OUTPUT_SLOT,
};

use coflow_api::{
    ArtifactSet, CodeGenerator, DataExporter, DecodedOutputOptions, Diagnostic, DiagnosticSet,
    Label, Severity, SourceLocation,
};
use coflow_project::{OutputConfig, Project};
use coflow_runtime::{BuildProjectSession, ProjectSchemaSession};
use serde_json::Value;
use staging::stage_artifact_set;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

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

#[derive(Debug)]
pub struct ReleasedOutput {
    pub provider_id: String,
    pub display_name: &'static str,
    pub dir: PathBuf,
}

#[derive(Debug)]
pub struct ArtifactReleaseReport {
    outputs: BTreeMap<&'static str, ReleasedOutput>,
}

impl ArtifactReleaseReport {
    pub fn output(&self, slot: &'static str) -> Result<&ReleasedOutput, DiagnosticSet> {
        self.outputs.get(slot).ok_or_else(|| {
            diagnostic_set(
                PathBuf::from(slot),
                format!("artifact release did not publish required `{slot}` output"),
            )
        })
    }
}

pub(crate) enum ArtifactGenerator<'a> {
    Data {
        session: &'a BuildProjectSession,
        exporter: Arc<dyn DataExporter>,
        options: DecodedOutputOptions,
    },
    BuildCode {
        session: &'a BuildProjectSession,
        codegen: Arc<dyn CodeGenerator>,
        options: DecodedOutputOptions,
        data_format: &'a str,
        id_as_enum_variants: &'a Value,
        include_model: bool,
    },
    SchemaCode {
        session: &'a ProjectSchemaSession,
        codegen: Arc<dyn CodeGenerator>,
        options: DecodedOutputOptions,
        data_format: &'a str,
        id_as_enum_variants: &'a Value,
    },
}

pub(crate) struct ArtifactOutputTarget {
    slot: &'static str,
    provider_id: String,
    display_name: &'static str,
    dir: PathBuf,
}

impl ArtifactOutputTarget {
    pub(crate) fn new(
        slot: &'static str,
        provider_id: impl Into<String>,
        display_name: &'static str,
        dir: PathBuf,
    ) -> Self {
        Self {
            slot,
            provider_id: provider_id.into(),
            display_name,
            dir,
        }
    }
}

pub(crate) struct ArtifactReleaseOutput<'a> {
    target: ArtifactOutputTarget,
    generator: ArtifactGenerator<'a>,
}

impl<'a> ArtifactReleaseOutput<'a> {
    pub(crate) const fn new(
        target: ArtifactOutputTarget,
        generator: ArtifactGenerator<'a>,
    ) -> Self {
        Self { target, generator }
    }
}

struct GeneratedArtifactOutput {
    slot: &'static str,
    provider_id: String,
    display_name: &'static str,
    dir: PathBuf,
    artifacts: ArtifactSet,
}

pub struct ArtifactReleasePlan<'a> {
    project: &'a Project,
    outputs: Vec<ArtifactReleaseOutput<'a>>,
    removed_outputs: Vec<&'static str>,
    enum_lock_update: EnumLockUpdate,
}

pub struct PreparedArtifactRelease<'a> {
    project: &'a Project,
    outputs: Vec<GeneratedArtifactOutput>,
    removed_outputs: Vec<&'static str>,
    enum_lock_update: EnumLockUpdate,
}

impl<'a> ArtifactReleasePlan<'a> {
    #[must_use]
    pub const fn new(project: &'a Project) -> Self {
        Self {
            project,
            outputs: Vec::new(),
            removed_outputs: Vec::new(),
            enum_lock_update: EnumLockUpdate::Preserve,
        }
    }

    pub(crate) fn add_output(&mut self, output: ArtifactReleaseOutput<'a>) {
        self.outputs.push(output);
    }

    pub fn remove_output(&mut self, slot: &'static str) {
        self.removed_outputs.push(slot);
    }

    pub fn replace_enum_lock(&mut self, lock: Value) {
        self.enum_lock_update = EnumLockUpdate::Replace(lock);
    }

    /// Validate output paths and generate every artifact set in memory.
    pub fn prepare(self) -> Result<PreparedArtifactRelease<'a>, DiagnosticSet> {
        let output_plans = self
            .outputs
            .iter()
            .map(|output| {
                crate::commands::artifact_safety::ArtifactOutputPlan::new(
                    match output.target.slot {
                        DATA_OUTPUT_SLOT => "outputs.data.dir",
                        CODE_OUTPUT_SLOT => "outputs.code.dir",
                        other => other,
                    },
                    output.target.dir.clone(),
                )
            })
            .collect::<Vec<_>>();
        let diagnostics = crate::commands::artifact_safety::artifact_safety_diagnostics(
            self.project,
            &output_plans,
        );
        if !diagnostics.is_empty() {
            return Err(diagnostics);
        }

        let mut outputs = Vec::with_capacity(self.outputs.len());
        for output in self.outputs {
            let artifacts = match &output.generator {
                ArtifactGenerator::Data {
                    session,
                    exporter,
                    options,
                } => session.export_artifacts(exporter.as_ref(), options),
                ArtifactGenerator::BuildCode {
                    session,
                    codegen,
                    options,
                    data_format,
                    id_as_enum_variants,
                    include_model,
                } => session.codegen_artifacts(
                    codegen.as_ref(),
                    options,
                    data_format,
                    id_as_enum_variants,
                    *include_model,
                ),
                ArtifactGenerator::SchemaCode {
                    session,
                    codegen,
                    options,
                    data_format,
                    id_as_enum_variants,
                } => session.codegen_artifacts(
                    codegen.as_ref(),
                    options,
                    data_format,
                    id_as_enum_variants,
                ),
            }?;
            outputs.push(GeneratedArtifactOutput {
                slot: output.target.slot,
                provider_id: output.target.provider_id,
                display_name: output.target.display_name,
                dir: output.target.dir,
                artifacts,
            });
        }

        Ok(PreparedArtifactRelease {
            project: self.project,
            outputs,
            removed_outputs: self.removed_outputs,
            enum_lock_update: self.enum_lock_update,
        })
    }

    /// Validate, generate, stage, and atomically publish every planned output.
    pub fn execute(self) -> Result<ArtifactReleaseReport, DiagnosticSet> {
        self.prepare()?.publish()
    }
}

impl PreparedArtifactRelease<'_> {
    /// Stage and atomically publish the already generated artifact sets.
    pub fn publish(self) -> Result<ArtifactReleaseReport, DiagnosticSet> {
        let mut staged = Vec::with_capacity(self.outputs.len());
        let mut metadata = Vec::with_capacity(self.outputs.len());
        for output in self.outputs {
            let staged_output = stage_artifact_set(&output.dir, output.artifacts)?;
            staged.push((output.slot, staged_output));
            metadata.push((output.slot, output.provider_id, output.display_name));
        }

        let published = publish_artifacts(
            self.project,
            staged,
            &self.removed_outputs,
            self.enum_lock_update,
        )?;
        let mut outputs = BTreeMap::new();
        for (slot, provider_id, display_name) in metadata {
            outputs.insert(
                slot,
                ReleasedOutput {
                    provider_id,
                    display_name,
                    dir: published.output_dir(slot)?.to_path_buf(),
                },
            );
        }
        Ok(ArtifactReleaseReport { outputs })
    }
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

pub fn output_options(output: &OutputConfig) -> Value {
    output.options().clone()
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
