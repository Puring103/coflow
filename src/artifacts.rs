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
    Label, LoaderGenerator, Severity, SourceLocation,
};
use coflow_project::{OutputConfig, Project};
use coflow_runtime::{BuildProjectSession, ProjectSchemaSession};
use serde_json::Value;
use staging::stage_artifact_set;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub(crate) fn clean_history(project: &Project) -> Result<(usize, usize), DiagnosticSet> {
    publication::clean_history(project)
}

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
    outputs: BTreeMap<String, ReleasedOutput>,
}

impl ArtifactReleaseReport {
    pub fn output(&self, slot: &str) -> Result<&ReleasedOutput, DiagnosticSet> {
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
    #[cfg(test)]
    BuildCode {
        session: &'a BuildProjectSession,
        codegen: Arc<dyn CodeGenerator>,
        options: &'a Value,
        id_as_enum_variants: &'a Value,
    },
    BuildCodeWithLoader {
        session: &'a BuildProjectSession,
        codegen: Arc<dyn CodeGenerator>,
        loader: Arc<dyn LoaderGenerator>,
        exporter: Arc<dyn DataExporter>,
        code_options: &'a Value,
        data_options: &'a Value,
        loader_options: &'a Value,
        id_as_enum_variants: &'a Value,
        object_layout: bool,
    },
    SchemaCodeWithLoader {
        session: &'a ProjectSchemaSession,
        codegen: Arc<dyn CodeGenerator>,
        loader: Arc<dyn LoaderGenerator>,
        exporter: Arc<dyn DataExporter>,
        code_options: &'a Value,
        data_options: &'a Value,
        loader_options: &'a Value,
        id_as_enum_variants: &'a Value,
        object_layout: bool,
    },
}

struct ArtifactReleaseOutput<'a> {
    slot: String,
    dir: PathBuf,
    generator: ArtifactGenerator<'a>,
}

struct GeneratedArtifactOutput {
    slot: String,
    provider_id: String,
    display_name: &'static str,
    dir: PathBuf,
    artifacts: ArtifactSet,
}

struct ValidatedArtifactReleaseOutput<'a> {
    slot: String,
    dir: PathBuf,
    generator: ValidatedArtifactGenerator<'a>,
}

enum ValidatedArtifactGenerator<'a> {
    Data {
        session: &'a BuildProjectSession,
        exporter: Arc<dyn DataExporter>,
        options: DecodedOutputOptions,
    },
    #[cfg(test)]
    BuildCode {
        session: &'a BuildProjectSession,
        codegen: Arc<dyn CodeGenerator>,
        options: DecodedOutputOptions,
        id_as_enum_variants: &'a Value,
        needs_model_for_build: bool,
    },
    BuildCodeWithLoader {
        session: &'a BuildProjectSession,
        codegen: Arc<dyn CodeGenerator>,
        loader: Arc<dyn LoaderGenerator>,
        code_options: DecodedOutputOptions,
        data_options: DecodedOutputOptions,
        loader_options: DecodedOutputOptions,
        id_as_enum_variants: &'a Value,
        needs_model_for_build: bool,
        object_layout: bool,
    },
    SchemaCodeWithLoader {
        session: &'a ProjectSchemaSession,
        codegen: Arc<dyn CodeGenerator>,
        loader: Arc<dyn LoaderGenerator>,
        code_options: DecodedOutputOptions,
        data_options: DecodedOutputOptions,
        loader_options: DecodedOutputOptions,
        id_as_enum_variants: &'a Value,
        object_layout: bool,
    },
}

pub struct ArtifactReleasePlan<'a> {
    project: &'a Project,
    outputs: Vec<ArtifactReleaseOutput<'a>>,
    removed_outputs: Vec<String>,
    enum_lock_update: EnumLockUpdate,
}

pub struct PreparedArtifactRelease<'a> {
    project: &'a Project,
    outputs: Vec<GeneratedArtifactOutput>,
    removed_outputs: Vec<String>,
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

    #[cfg(test)]
    pub(crate) fn add_data(
        &mut self,
        session: &'a BuildProjectSession,
        exporter: Arc<dyn DataExporter>,
        output: &'a OutputConfig,
        override_dir: Option<&Path>,
    ) {
        self.add_data_for_slot(DATA_OUTPUT_SLOT, session, exporter, output, override_dir);
    }

    pub(crate) fn add_data_for_slot(
        &mut self,
        slot: impl Into<String>,
        session: &'a BuildProjectSession,
        exporter: Arc<dyn DataExporter>,
        output: &'a OutputConfig,
        override_dir: Option<&Path>,
    ) {
        self.outputs.push(ArtifactReleaseOutput {
            slot: slot.into(),
            dir: output_dir(self.project, output, override_dir),
            generator: ArtifactGenerator::Data {
                session,
                exporter,
                options: output.options(),
            },
        });
    }

    #[cfg(test)]
    pub(crate) fn add_build_code(
        &mut self,
        session: &'a BuildProjectSession,
        codegen: Arc<dyn CodeGenerator>,
        output: &'a OutputConfig,
        override_dir: Option<&Path>,
        id_as_enum_variants: &'a Value,
    ) {
        self.add_build_code_for_slot(
            CODE_OUTPUT_SLOT,
            session,
            codegen,
            output,
            override_dir,
            id_as_enum_variants,
        );
    }

    #[cfg(test)]
    pub(crate) fn add_build_code_for_slot(
        &mut self,
        slot: impl Into<String>,
        session: &'a BuildProjectSession,
        codegen: Arc<dyn CodeGenerator>,
        output: &'a OutputConfig,
        override_dir: Option<&Path>,
        id_as_enum_variants: &'a Value,
    ) {
        self.outputs.push(ArtifactReleaseOutput {
            slot: slot.into(),
            dir: output_dir(self.project, output, override_dir),
            generator: ArtifactGenerator::BuildCode {
                session,
                codegen,
                options: output.options(),
                id_as_enum_variants,
            },
        });
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn add_build_code_with_loader_for_slot(
        &mut self,
        slot: impl Into<String>,
        session: &'a BuildProjectSession,
        codegen: Arc<dyn CodeGenerator>,
        loader: Arc<dyn LoaderGenerator>,
        exporter: Arc<dyn DataExporter>,
        code_output: &'a OutputConfig,
        data_output: &'a OutputConfig,
        loader_options: &'a Value,
        override_dir: Option<&Path>,
        id_as_enum_variants: &'a Value,
    ) {
        self.outputs.push(ArtifactReleaseOutput {
            slot: slot.into(),
            dir: output_dir(self.project, code_output, override_dir),
            generator: ArtifactGenerator::BuildCodeWithLoader {
                session,
                codegen,
                loader,
                exporter,
                code_options: code_output.options(),
                data_options: data_output.options(),
                loader_options,
                id_as_enum_variants,
                object_layout: self.project.config.outputs.is_object_shape(),
            },
        });
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn add_schema_code_with_loader_for_slot(
        &mut self,
        slot: impl Into<String>,
        session: &'a ProjectSchemaSession,
        codegen: Arc<dyn CodeGenerator>,
        loader: Arc<dyn LoaderGenerator>,
        exporter: Arc<dyn DataExporter>,
        code_output: &'a OutputConfig,
        data_output: &'a OutputConfig,
        loader_options: &'a Value,
        override_dir: Option<&Path>,
        id_as_enum_variants: &'a Value,
    ) {
        self.outputs.push(ArtifactReleaseOutput {
            slot: slot.into(),
            dir: output_dir(self.project, code_output, override_dir),
            generator: ArtifactGenerator::SchemaCodeWithLoader {
                session,
                codegen,
                loader,
                exporter,
                code_options: code_output.options(),
                data_options: data_output.options(),
                loader_options,
                id_as_enum_variants,
                object_layout: self.project.config.outputs.is_object_shape(),
            },
        });
    }

    pub fn remove_output(&mut self, slot: impl Into<String>) {
        self.removed_outputs.push(slot.into());
    }

    pub fn remove_stale_managed_outputs(
        &mut self,
        planned_slots: &BTreeSet<String>,
    ) -> Result<(), DiagnosticSet> {
        for slot in publication::active_output_slots(self.project)? {
            if is_managed_output_slot(&slot) && !planned_slots.contains(&slot) {
                self.remove_output(slot);
            }
        }
        Ok(())
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
            .map(|output| safety::ArtifactOutputPlan::new(output.slot.clone(), output.dir.clone()))
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
        _project: &Project,
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
            #[cfg(test)]
            ArtifactGenerator::BuildCode {
                session,
                codegen,
                options,
                id_as_enum_variants,
            } => {
                let descriptor = codegen.descriptor();
                ValidatedArtifactGenerator::BuildCode {
                    session,
                    options: codegen.decode_options(options)?,
                    codegen,
                    id_as_enum_variants,
                    needs_model_for_build: descriptor.needs_model_for_build,
                }
            }
            ArtifactGenerator::BuildCodeWithLoader {
                session,
                codegen,
                loader,
                exporter,
                code_options,
                data_options,
                loader_options,
                id_as_enum_variants,
                object_layout,
            } => {
                let needs_model_for_build = codegen.descriptor().needs_model_for_build;
                ValidatedArtifactGenerator::BuildCodeWithLoader {
                    session,
                    code_options: codegen.decode_options(code_options)?,
                    data_options: exporter.decode_options(data_options)?,
                    loader_options: loader.decode_options(loader_options)?,
                    codegen,
                    loader,
                    id_as_enum_variants,
                    needs_model_for_build,
                    object_layout,
                }
            }
            ArtifactGenerator::SchemaCodeWithLoader {
                session,
                codegen,
                loader,
                exporter,
                code_options,
                data_options,
                loader_options,
                id_as_enum_variants,
                object_layout,
            } => ValidatedArtifactGenerator::SchemaCodeWithLoader {
                session,
                code_options: codegen.decode_options(code_options)?,
                data_options: exporter.decode_options(data_options)?,
                loader_options: loader.decode_options(loader_options)?,
                codegen,
                loader,
                id_as_enum_variants,
                object_layout,
            },
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
            #[cfg(test)]
            ValidatedArtifactGenerator::BuildCode {
                session,
                codegen,
                options,
                id_as_enum_variants,
                needs_model_for_build,
            } => {
                let descriptor = codegen.descriptor();
                let artifacts = session.codegen_artifacts(
                    codegen.as_ref(),
                    &options,
                    id_as_enum_variants,
                    needs_model_for_build,
                )?;
                (
                    descriptor.id.to_string(),
                    descriptor.display_name,
                    artifacts,
                )
            }
            ValidatedArtifactGenerator::BuildCodeWithLoader {
                session,
                codegen,
                loader,
                code_options,
                data_options,
                loader_options,
                id_as_enum_variants,
                needs_model_for_build,
                object_layout,
            } => {
                let descriptor = codegen.descriptor();
                let common = session.codegen_artifacts(
                    codegen.as_ref(),
                    &code_options,
                    id_as_enum_variants,
                    needs_model_for_build,
                )?;
                let loader_artifacts = session.loader_artifacts(
                    loader.as_ref(),
                    &code_options,
                    &data_options,
                    &loader_options,
                    id_as_enum_variants,
                )?;
                let artifacts = merge_code_and_loader_artifacts(
                    loader.as_ref(),
                    common,
                    loader_artifacts,
                    object_layout,
                    &self.slot,
                )?;
                (
                    descriptor.id.to_string(),
                    descriptor.display_name,
                    artifacts,
                )
            }
            ValidatedArtifactGenerator::SchemaCodeWithLoader {
                session,
                codegen,
                loader,
                code_options,
                data_options,
                loader_options,
                id_as_enum_variants,
                object_layout,
            } => {
                let descriptor = codegen.descriptor();
                let common = session.codegen_artifacts(
                    codegen.as_ref(),
                    &code_options,
                    id_as_enum_variants,
                )?;
                let loader_artifacts = session.loader_artifacts(
                    loader.as_ref(),
                    &code_options,
                    &data_options,
                    &loader_options,
                    id_as_enum_variants,
                )?;
                let artifacts = merge_code_and_loader_artifacts(
                    loader.as_ref(),
                    common,
                    loader_artifacts,
                    object_layout,
                    &self.slot,
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

fn merge_code_and_loader_artifacts(
    loader: &dyn LoaderGenerator,
    common: ArtifactSet,
    loader_artifacts: ArtifactSet,
    object_layout: bool,
    slot: &str,
) -> Result<ArtifactSet, DiagnosticSet> {
    if object_layout {
        loader.merge_object_layout_artifacts(common, loader_artifacts)
    } else {
        let mut files = common.into_files();
        files.extend(loader_artifacts.into_files());
        ArtifactSet::new(files).map_err(|error| {
            diagnostic_set(
                PathBuf::from(slot),
                format!("generated code and loader artifacts conflict: {error}"),
            )
        })
    }
}

fn validate_release_slots(
    outputs: &[ArtifactReleaseOutput<'_>],
    removed_outputs: &[String],
) -> Result<(), DiagnosticSet> {
    let mut output_slots = BTreeSet::new();
    for output in outputs {
        if output.slot.is_empty() || !output_slots.insert(output.slot.as_str()) {
            return Err(diagnostic_set(
                PathBuf::from(&output.slot),
                format!(
                    "artifact release contains invalid or duplicate `{}` output",
                    output.slot
                ),
            ));
        }
    }
    let mut removed_slots = BTreeSet::new();
    for slot in removed_outputs {
        if slot.is_empty()
            || !removed_slots.insert(slot.as_str())
            || output_slots.contains(slot.as_str())
        {
            return Err(diagnostic_set(
                PathBuf::from(slot),
                format!("artifact release contains conflicting `{slot}` removal"),
            ));
        }
    }
    Ok(())
}

impl PreparedArtifactRelease<'_> {
    /// Stage and atomically publish the already generated artifact sets.
    pub fn publish(self) -> Result<ArtifactReleaseReport, DiagnosticSet> {
        let mut staged = Vec::with_capacity(self.outputs.len());
        let mut metadata = Vec::with_capacity(self.outputs.len());
        for output in self.outputs {
            let staged_output = stage_artifact_set(
                &publication::artifact_state_dir(self.project),
                &output.slot,
                &output.dir,
                output.artifacts,
            )?;
            staged.push((output.slot.clone(), staged_output));
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
            let dir = published.output_dir(&slot)?.to_path_buf();
            outputs.insert(
                slot,
                ReleasedOutput {
                    provider_id,
                    display_name,
                    dir,
                },
            );
        }
        Ok(ArtifactReleaseReport { outputs })
    }
}

pub fn data_output_slot(target_index: usize) -> String {
    if target_index == 0 {
        DATA_OUTPUT_SLOT.to_string()
    } else {
        format!("output-{target_index}-data")
    }
}

pub fn code_output_slot(target_index: usize) -> String {
    if target_index == 0 {
        CODE_OUTPUT_SLOT.to_string()
    } else {
        format!("output-{target_index}-code")
    }
}

fn is_managed_output_slot(slot: &str) -> bool {
    if matches!(slot, DATA_OUTPUT_SLOT | CODE_OUTPUT_SLOT) {
        return true;
    }
    let Some(rest) = slot.strip_prefix("output-") else {
        return false;
    };
    let Some((index, kind)) = rest.rsplit_once('-') else {
        return false;
    };
    index.parse::<usize>().is_ok() && matches!(kind, "data" | "code")
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
                .targets()
                .first()
                .map(|target| &target.data)
                .expect("data output")
        }

        fn code_output(&self) -> &coflow_project::OutputConfig {
            self.project
                .config
                .outputs
                .targets()
                .first()
                .and_then(|target| target.code.as_ref())
                .expect("code output")
        }
    }

    impl Drop for ArtifactFixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
}
