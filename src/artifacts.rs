mod fault;
mod publication;
mod safety;
mod staging;

pub(crate) use safety::artifact_diagnostic_set;

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
use std::collections::{BTreeMap, BTreeSet};
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

enum ArtifactGenerator<'a> {
    Data {
        session: &'a BuildProjectSession,
        exporter: Arc<dyn DataExporter>,
        options: &'a Value,
    },
    BuildCode {
        session: &'a BuildProjectSession,
        codegen: Arc<dyn CodeGenerator>,
        options: &'a Value,
        data_format: &'a str,
        id_as_enum_variants: &'a Value,
    },
    SchemaCode {
        session: &'a ProjectSchemaSession,
        codegen: Arc<dyn CodeGenerator>,
        options: &'a Value,
        data_format: &'a str,
        id_as_enum_variants: &'a Value,
    },
}

struct ArtifactReleaseOutput<'a> {
    slot: &'static str,
    dir: PathBuf,
    generator: ArtifactGenerator<'a>,
}

struct GeneratedArtifactOutput {
    slot: &'static str,
    provider_id: String,
    display_name: &'static str,
    dir: PathBuf,
    artifacts: ArtifactSet,
}

struct ValidatedArtifactReleaseOutput<'a> {
    slot: &'static str,
    dir: PathBuf,
    generator: ValidatedArtifactGenerator<'a>,
}

enum ValidatedArtifactGenerator<'a> {
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
        needs_model_for_build: bool,
    },
    SchemaCode {
        session: &'a ProjectSchemaSession,
        codegen: Arc<dyn CodeGenerator>,
        options: DecodedOutputOptions,
        data_format: &'a str,
        id_as_enum_variants: &'a Value,
    },
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

    pub(crate) fn add_data(
        &mut self,
        session: &'a BuildProjectSession,
        exporter: Arc<dyn DataExporter>,
        output: &'a OutputConfig,
        override_dir: Option<&Path>,
    ) {
        self.outputs.push(ArtifactReleaseOutput {
            slot: DATA_OUTPUT_SLOT,
            dir: output_dir(self.project, output, override_dir),
            generator: ArtifactGenerator::Data {
                session,
                exporter,
                options: output.options(),
            },
        });
    }

    pub(crate) fn add_build_code(
        &mut self,
        session: &'a BuildProjectSession,
        codegen: Arc<dyn CodeGenerator>,
        output: &'a OutputConfig,
        override_dir: Option<&Path>,
        data_format: &'a str,
        id_as_enum_variants: &'a Value,
    ) {
        self.outputs.push(ArtifactReleaseOutput {
            slot: CODE_OUTPUT_SLOT,
            dir: output_dir(self.project, output, override_dir),
            generator: ArtifactGenerator::BuildCode {
                session,
                codegen,
                options: output.options(),
                data_format,
                id_as_enum_variants,
            },
        });
    }

    pub(crate) fn add_schema_code(
        &mut self,
        session: &'a ProjectSchemaSession,
        codegen: Arc<dyn CodeGenerator>,
        output: &'a OutputConfig,
        override_dir: Option<&Path>,
        data_format: &'a str,
        id_as_enum_variants: &'a Value,
    ) {
        self.outputs.push(ArtifactReleaseOutput {
            slot: CODE_OUTPUT_SLOT,
            dir: output_dir(self.project, output, override_dir),
            generator: ArtifactGenerator::SchemaCode {
                session,
                codegen,
                options: output.options(),
                data_format,
                id_as_enum_variants,
            },
        });
    }

    pub fn remove_output(&mut self, slot: &'static str) {
        self.removed_outputs.push(slot);
    }

    pub fn replace_enum_lock(&mut self, lock: Value) {
        self.enum_lock_update = EnumLockUpdate::Replace(lock);
    }

    /// Validate every output configuration, validate artifact safety, and
    /// generate every artifact set in memory.
    pub fn prepare(self) -> Result<PreparedArtifactRelease<'a>, DiagnosticSet> {
        let Self {
            project,
            outputs,
            removed_outputs,
            enum_lock_update,
        } = self;
        validate_release_slots(&outputs, &removed_outputs)?;
        let validated_outputs = validate_outputs(project, outputs)?;
        let output_plans = validated_outputs
            .iter()
            .map(|output| {
                safety::ArtifactOutputPlan::new(
                    match output.slot {
                        DATA_OUTPUT_SLOT => "outputs.data.dir",
                        CODE_OUTPUT_SLOT => "outputs.code.dir",
                        other => other,
                    },
                    output.dir.clone(),
                )
            })
            .collect::<Vec<_>>();
        let diagnostics = safety::artifact_safety_diagnostics(project, &output_plans);
        if !diagnostics.is_empty() {
            return Err(diagnostics);
        }

        let mut outputs = Vec::with_capacity(validated_outputs.len());
        for output in validated_outputs {
            outputs.push(output.generate()?);
        }

        Ok(PreparedArtifactRelease {
            project,
            outputs,
            removed_outputs,
            enum_lock_update,
        })
    }

    /// Validate, generate, stage, and atomically publish every planned output.
    pub fn execute(self) -> Result<ArtifactReleaseReport, DiagnosticSet> {
        self.prepare()?.publish()
    }
}

fn validate_outputs<'a>(
    project: &'a Project,
    outputs: Vec<ArtifactReleaseOutput<'a>>,
) -> Result<Vec<ValidatedArtifactReleaseOutput<'a>>, DiagnosticSet> {
    let mut diagnostics = DiagnosticSet::empty();
    let mut validated = Vec::with_capacity(outputs.len());
    for output in outputs {
        match output.validate(project) {
            Ok(output) => validated.push(output),
            Err(output_diagnostics) => diagnostics.extend(output_diagnostics),
        }
    }
    if diagnostics.is_empty() {
        Ok(validated)
    } else {
        Err(diagnostics)
    }
}

impl<'a> ArtifactReleaseOutput<'a> {
    fn validate(
        self,
        project: &Project,
    ) -> Result<ValidatedArtifactReleaseOutput<'a>, DiagnosticSet> {
        let generator = match self.generator {
            ArtifactGenerator::Data {
                session,
                exporter,
                options,
            } => ValidatedArtifactGenerator::Data {
                session,
                options: exporter.decode_options(options)?,
                exporter,
            },
            ArtifactGenerator::BuildCode {
                session,
                codegen,
                options,
                data_format,
                id_as_enum_variants,
            } => {
                let descriptor = validate_codegen(project, codegen.as_ref(), data_format)?;
                ValidatedArtifactGenerator::BuildCode {
                    session,
                    options: codegen.decode_options(options)?,
                    codegen,
                    data_format,
                    id_as_enum_variants,
                    needs_model_for_build: descriptor.needs_model_for_build,
                }
            }
            ArtifactGenerator::SchemaCode {
                session,
                codegen,
                options,
                data_format,
                id_as_enum_variants,
            } => {
                validate_codegen(project, codegen.as_ref(), data_format)?;
                ValidatedArtifactGenerator::SchemaCode {
                    session,
                    options: codegen.decode_options(options)?,
                    codegen,
                    data_format,
                    id_as_enum_variants,
                }
            }
        };
        Ok(ValidatedArtifactReleaseOutput {
            slot: self.slot,
            dir: self.dir,
            generator,
        })
    }
}

impl ValidatedArtifactReleaseOutput<'_> {
    fn generate(self) -> Result<GeneratedArtifactOutput, DiagnosticSet> {
        let (provider_id, display_name, artifacts) = match self.generator {
            ValidatedArtifactGenerator::Data {
                session,
                exporter,
                options,
            } => {
                let descriptor = exporter.descriptor();
                let artifacts = session.export_artifacts(exporter.as_ref(), &options)?;
                (
                    descriptor.id.to_string(),
                    descriptor.display_name,
                    artifacts,
                )
            }
            ValidatedArtifactGenerator::BuildCode {
                session,
                codegen,
                options,
                data_format,
                id_as_enum_variants,
                needs_model_for_build,
            } => {
                let descriptor = codegen.descriptor();
                let artifacts = session.codegen_artifacts(
                    codegen.as_ref(),
                    &options,
                    data_format,
                    id_as_enum_variants,
                    needs_model_for_build,
                )?;
                (
                    descriptor.id.to_string(),
                    descriptor.display_name,
                    artifacts,
                )
            }
            ValidatedArtifactGenerator::SchemaCode {
                session,
                codegen,
                options,
                data_format,
                id_as_enum_variants,
            } => {
                let descriptor = codegen.descriptor();
                let artifacts = session.codegen_artifacts(
                    codegen.as_ref(),
                    &options,
                    data_format,
                    id_as_enum_variants,
                )?;
                (
                    descriptor.id.to_string(),
                    descriptor.display_name,
                    artifacts,
                )
            }
        };
        Ok(GeneratedArtifactOutput {
            slot: self.slot,
            provider_id,
            display_name,
            dir: self.dir,
            artifacts,
        })
    }
}

fn validate_release_slots(
    outputs: &[ArtifactReleaseOutput<'_>],
    removed_outputs: &[&str],
) -> Result<(), DiagnosticSet> {
    let mut output_slots = BTreeSet::new();
    for output in outputs {
        if output.slot.is_empty() || !output_slots.insert(output.slot) {
            return Err(diagnostic_set(
                PathBuf::from(output.slot),
                format!(
                    "artifact release contains invalid or duplicate `{}` output",
                    output.slot
                ),
            ));
        }
    }
    let mut removed_slots = BTreeSet::new();
    for slot in removed_outputs {
        if slot.is_empty() || !removed_slots.insert(*slot) || output_slots.contains(*slot) {
            return Err(diagnostic_set(
                PathBuf::from(slot),
                format!("artifact release contains conflicting `{slot}` removal"),
            ));
        }
    }
    Ok(())
}

fn validate_codegen(
    project: &Project,
    codegen: &dyn CodeGenerator,
    data_format: &str,
) -> Result<&'static coflow_api::CodegenDescriptor, DiagnosticSet> {
    let descriptor = codegen.descriptor();
    if descriptor.supported_data_formats.contains(&data_format) {
        Ok(descriptor)
    } else {
        Err(project_config_diagnostic_set(
            project,
            format!(
                "code generator `{}` does not support data format `{data_format}`",
                descriptor.id
            ),
            ["outputs", "code", "type"],
        ))
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

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::{ArtifactReleasePlan, DATA_OUTPUT_SLOT};
    use coflow_api::{
        ArtifactContentKind, ArtifactFile, ArtifactSet, CodeGenerator, CodegenContext,
        CodegenDescriptor, DataExporter, DecodedOutputOptions, Diagnostic, DiagnosticSet,
        ExportContext, ExporterDescriptor, ProviderRegistry,
    };
    use coflow_project::Project;
    use coflow_runtime::Runtime;
    use serde_json::Value;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    static TEST_EXPORTER_DESCRIPTOR: ExporterDescriptor = ExporterDescriptor {
        id: "test-data",
        display_name: "Test data",
        table_file_extension: "txt",
        content_kind: ArtifactContentKind::Text,
    };
    static TEST_CODEGEN_DESCRIPTOR: CodegenDescriptor = CodegenDescriptor {
        id: "test-code",
        display_name: "Test code",
        language: "test",
        file_extensions: &["txt"],
        supported_data_formats: &["test-data"],
        needs_model_for_build: false,
    };

    #[derive(Debug)]
    struct TestExporter {
        decode_calls: Arc<AtomicUsize>,
        export_calls: Arc<AtomicUsize>,
    }

    impl DataExporter for TestExporter {
        fn descriptor(&self) -> &'static ExporterDescriptor {
            &TEST_EXPORTER_DESCRIPTOR
        }

        fn decode_options(&self, _options: &Value) -> Result<DecodedOutputOptions, DiagnosticSet> {
            self.decode_calls.fetch_add(1, Ordering::SeqCst);
            Ok(DecodedOutputOptions::new(TEST_EXPORTER_DESCRIPTOR.id, ()))
        }

        fn export(
            &self,
            _ctx: ExportContext<'_>,
            _options: &DecodedOutputOptions,
        ) -> Result<ArtifactSet, DiagnosticSet> {
            self.export_calls.fetch_add(1, Ordering::SeqCst);
            ArtifactSet::new(vec![ArtifactFile::text("data.txt", "data")]).map_err(|error| {
                DiagnosticSet::one(Diagnostic::error(
                    "TEST-ARTIFACT",
                    "TEST",
                    error.to_string(),
                ))
            })
        }
    }

    #[derive(Debug)]
    struct FailingCodegen;

    impl CodeGenerator for FailingCodegen {
        fn descriptor(&self) -> &'static CodegenDescriptor {
            &TEST_CODEGEN_DESCRIPTOR
        }

        fn decode_options(&self, _options: &Value) -> Result<DecodedOutputOptions, DiagnosticSet> {
            Ok(DecodedOutputOptions::new(TEST_CODEGEN_DESCRIPTOR.id, ()))
        }

        fn generate(
            &self,
            _ctx: CodegenContext<'_>,
            _options: &DecodedOutputOptions,
        ) -> Result<ArtifactSet, DiagnosticSet> {
            Err(DiagnosticSet::one(Diagnostic::error(
                "TEST-CODEGEN",
                "TEST",
                "injected code generation failure",
            )))
        }
    }

    #[derive(Debug)]
    struct FailingOptionCodegen;

    impl CodeGenerator for FailingOptionCodegen {
        fn descriptor(&self) -> &'static CodegenDescriptor {
            &TEST_CODEGEN_DESCRIPTOR
        }

        fn decode_options(&self, _options: &Value) -> Result<DecodedOutputOptions, DiagnosticSet> {
            Err(DiagnosticSet::one(Diagnostic::error(
                "TEST-CODEGEN-OPTIONS",
                "TEST",
                "injected code generation option failure",
            )))
        }

        fn generate(
            &self,
            _ctx: CodegenContext<'_>,
            _options: &DecodedOutputOptions,
        ) -> Result<ArtifactSet, DiagnosticSet> {
            Err(DiagnosticSet::one(Diagnostic::error(
                "TEST-CODEGEN",
                "TEST",
                "injected code generation failure",
            )))
        }
    }

    #[test]
    fn slot_conflicts_fail_before_provider_option_decoding() {
        let fixture = ArtifactFixture::new("slot-conflict");
        let decode_calls = Arc::new(AtomicUsize::new(0));
        let exporter = Arc::new(TestExporter {
            decode_calls: Arc::clone(&decode_calls),
            export_calls: Arc::new(AtomicUsize::new(0)),
        });
        let mut release = ArtifactReleasePlan::new(&fixture.project);
        release.add_data(&fixture.session, exporter, fixture.data_output(), None);
        release.remove_output(DATA_OUTPUT_SLOT);

        let diagnostics = release.prepare().err().expect("conflicting release");

        assert!(diagnostics
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("conflicting `data` removal")));
        assert_eq!(decode_calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn later_generation_failure_does_not_stage_earlier_output() {
        let fixture = ArtifactFixture::new("generation-failure");
        let exporter = Arc::new(TestExporter {
            decode_calls: Arc::new(AtomicUsize::new(0)),
            export_calls: Arc::new(AtomicUsize::new(0)),
        });
        let mut release = ArtifactReleasePlan::new(&fixture.project);
        release.add_data(&fixture.session, exporter, fixture.data_output(), None);
        release.add_build_code(
            &fixture.session,
            Arc::new(FailingCodegen),
            fixture.code_output(),
            None,
            "test-data",
            &Value::Null,
        );

        let diagnostics = release.prepare().err().expect("code generation failure");

        assert!(diagnostics
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "TEST-CODEGEN"));
        assert!(!fixture.root.join("generated/data").exists());
        assert!(!fixture.root.join(".coflow/artifacts").exists());
    }

    #[test]
    fn later_option_failure_skips_every_output_generation() {
        let fixture = ArtifactFixture::new("option-failure");
        let decode_calls = Arc::new(AtomicUsize::new(0));
        let export_calls = Arc::new(AtomicUsize::new(0));
        let exporter = Arc::new(TestExporter {
            decode_calls: Arc::clone(&decode_calls),
            export_calls: Arc::clone(&export_calls),
        });
        let mut release = ArtifactReleasePlan::new(&fixture.project);
        release.add_data(&fixture.session, exporter, fixture.data_output(), None);
        release.add_build_code(
            &fixture.session,
            Arc::new(FailingOptionCodegen),
            fixture.code_output(),
            None,
            "test-data",
            &Value::Null,
        );

        let diagnostics = release.prepare().err().expect("code option failure");

        assert!(diagnostics
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "TEST-CODEGEN-OPTIONS"));
        assert_eq!(decode_calls.load(Ordering::SeqCst), 1);
        assert_eq!(export_calls.load(Ordering::SeqCst), 0);
        assert!(!fixture.root.join("generated/data").exists());
        assert!(!fixture.root.join(".coflow/artifacts").exists());
    }

    struct ArtifactFixture {
        root: std::path::PathBuf,
        project: Project,
        session: coflow_runtime::BuildProjectSession,
    }

    impl ArtifactFixture {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "coflow-release-{name}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .expect("system time")
                    .as_nanos()
            ));
            std::fs::create_dir_all(&root).expect("create project root");
            std::fs::write(root.join("schema.cft"), "type Item {}\n").expect("write schema");
            std::fs::write(
                root.join("coflow.yaml"),
                "schema: schema.cft\noutputs:\n  data:\n    type: test-data\n    dir: generated/data\n  code:\n    type: test-code\n    dir: generated/code\n",
            )
            .expect("write project config");
            let project = Project::open(Some(&root)).expect("open project");
            let session = Runtime::new(ProviderRegistry::default())
                .build_project_session(project.clone())
                .expect("build project session");
            assert!(!session.queries().has_diagnostics());
            Self {
                root,
                project,
                session,
            }
        }

        fn data_output(&self) -> &coflow_project::OutputConfig {
            self.project
                .config
                .outputs
                .data
                .as_ref()
                .expect("data output")
        }

        fn code_output(&self) -> &coflow_project::OutputConfig {
            self.project
                .config
                .outputs
                .code
                .as_ref()
                .expect("code output")
        }
    }

    impl Drop for ArtifactFixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
}
