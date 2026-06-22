#![cfg_attr(
    not(test),
    deny(
        clippy::dbg_macro,
        clippy::expect_used,
        clippy::panic,
        clippy::panic_in_result_fn,
        clippy::todo,
        clippy::unimplemented,
        clippy::unreachable,
        clippy::unwrap_used
    )
)]
#![allow(clippy::multiple_crate_versions)]

mod artifacts;
mod data;
mod schema;

use artifacts::{
    commit_staged_dir_and_file, commit_staged_dirs_and_file, configured_data_format,
    configured_data_output, output_dir, preflight_codegen, required_code_output,
    required_data_output, stage_codegen_artifacts, stage_data_tables, stage_json_file,
    write_data_tables, CodegenArtifactRequest,
};
use coflow_api::{Diagnostic, DiagnosticSet, Label, ProviderRegistry, Severity, SourceLocation};
use coflow_project::{Project, SourceLocationSpec};
use data::load_project_data;
use schema::compile_project_schema;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

const ENUM_LOCKFILE_NAME: &str = "coflow.enum.lock.json";
pub const JSON_EXPORTER_ID: &str = "json";
pub const MESSAGEPACK_EXPORTER_ID: &str = "messagepack";
pub const CSHARP_CODEGEN_ID: &str = "csharp";

#[derive(Debug)]
pub enum PipelineOutcome<T> {
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
/// Returns an error for project configuration errors or unrecoverable I/O and
/// artifact errors. Schema, data loading, data-model, and check diagnostics are
/// returned as `PipelineOutcome::Diagnostics`.
pub fn check_project(
    project: &Project,
    registry: &ProviderRegistry,
) -> Result<PipelineOutcome<CheckReport>, String> {
    let mut diagnostics = project.schema_diagnostic_set();
    diagnostics.extend(project.data_diagnostic_set());
    if !diagnostics.is_empty() {
        return Ok(PipelineOutcome::Diagnostics(diagnostics));
    }
    let schema = match compile_project_schema(project)? {
        Ok(schema) => schema,
        Err(diagnostics) => return Ok(PipelineOutcome::Diagnostics(diagnostics)),
    };
    match load_project_data(project, &schema, registry) {
        Ok(_) => Ok(PipelineOutcome::Success(CheckReport)),
        Err(diagnostics) => Ok(PipelineOutcome::Diagnostics(diagnostics)),
    }
}

/// Runs validation, data export, and configured code generation.
///
/// # Errors
///
/// Returns an error for invalid project/output configuration, unsupported
/// output targets, or artifact write/codegen failures. Schema, data loading,
/// data-model, and check diagnostics are returned as
/// `PipelineOutcome::Diagnostics`.
pub fn build_project(
    project: &Project,
    registry: &ProviderRegistry,
    options: BuildOptions<'_>,
) -> Result<PipelineOutcome<BuildReport>, String> {
    let diagnostics = build_config_diagnostics(project);
    if !diagnostics.is_empty() {
        return Ok(PipelineOutcome::Diagnostics(diagnostics));
    }
    let plan = match build_provider_plan(project, registry, options) {
        Ok(plan) => plan,
        Err(diagnostics) => return Ok(PipelineOutcome::Diagnostics(diagnostics)),
    };
    let schema = match compile_project_schema(project)? {
        Ok(schema) => schema,
        Err(diagnostics) => return Ok(PipelineOutcome::Diagnostics(diagnostics)),
    };
    let load_output = match load_project_data(project, &schema, registry) {
        Ok(output) => output,
        Err(diagnostics) => return Ok(PipelineOutcome::Diagnostics(diagnostics)),
    };

    let mut preflight_diagnostics =
        build_codegen_preflight_diagnostics(registry, &schema, &load_output.model, &plan)?;
    preflight_diagnostics.extend(artifact_safety_diagnostics(project, &plan.artifact_outputs));
    if !preflight_diagnostics.is_empty() {
        return Ok(PipelineOutcome::Diagnostics(preflight_diagnostics));
    }

    let staged_data = match stage_data_tables(
        registry,
        &schema,
        &load_output.model,
        plan.data.exporter_id,
        plan.data.output,
        &plan.data.dir,
    ) {
        Ok(staged_data) => staged_data,
        Err(diagnostics) => return Ok(PipelineOutcome::Diagnostics(diagnostics)),
    };
    let code = match commit_build_artifacts(
        project,
        registry,
        &schema,
        &load_output.model,
        staged_data,
        &plan,
    ) {
        Ok(code) => code,
        Err(diagnostics) => return Ok(PipelineOutcome::Diagnostics(diagnostics)),
    };

    let data = ExportReport {
        exporter_id: plan.data.exporter_id.to_string(),
        display_name: plan.data.display_name.to_string(),
        dir: plan.data.dir,
    };

    Ok(PipelineOutcome::Success(BuildReport { data, code }))
}

/// Exports project data in the requested format.
///
/// # Errors
///
/// Returns an error for invalid project/output configuration, unsupported data
/// format configuration, or artifact write failures. Schema, data loading,
/// data-model, and check diagnostics are returned as
/// `PipelineOutcome::Diagnostics`.
pub fn export_project_data(
    project: &Project,
    registry: &ProviderRegistry,
    exporter_id: &str,
    options: ExportOptions<'_>,
) -> Result<PipelineOutcome<ExportReport>, String> {
    let mut diagnostics = project.schema_diagnostic_set();
    diagnostics.extend(project.data_diagnostic_set());
    let command = format!("coflow export {exporter_id}");
    if let Err(message) = required_data_output(project, exporter_id, &command) {
        diagnostics.push(project_diagnostic(
            &project.config_path,
            message,
            ["outputs", "data"],
        ));
    }
    if !diagnostics.is_empty() {
        return Ok(PipelineOutcome::Diagnostics(diagnostics));
    }
    let Some(exporter) = registry.exporter(exporter_id) else {
        return Ok(PipelineOutcome::Diagnostics(project_diagnostic_set(
            &project.config_path,
            format!("no data exporter registered for `{exporter_id}`"),
            ["outputs", "data", "type"],
        )));
    };
    let exporter_descriptor = exporter.descriptor();
    let output = required_data_output(project, exporter_id, &command)?;
    let dir = output_dir(project, output, options.out_dir);
    let schema = match compile_project_schema(project)? {
        Ok(schema) => schema,
        Err(diagnostics) => return Ok(PipelineOutcome::Diagnostics(diagnostics)),
    };
    let load_output = match load_project_data(project, &schema, registry) {
        Ok(output) => output,
        Err(diagnostics) => return Ok(PipelineOutcome::Diagnostics(diagnostics)),
    };
    let artifact_diagnostics = artifact_safety_diagnostics(
        project,
        &[ArtifactOutputPlan::new("outputs.data.dir", dir.clone())],
    );
    if !artifact_diagnostics.is_empty() {
        return Ok(PipelineOutcome::Diagnostics(artifact_diagnostics));
    }
    if let Err(diagnostics) = write_data_tables(
        registry,
        &schema,
        &load_output.model,
        exporter_id,
        output,
        &dir,
    ) {
        return Ok(PipelineOutcome::Diagnostics(diagnostics));
    }
    Ok(PipelineOutcome::Success(ExportReport {
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
/// returned as `PipelineOutcome::Diagnostics`.
pub fn generate_project_code(
    project: &Project,
    registry: &ProviderRegistry,
    codegen_id: &str,
    options: CodegenOptions<'_>,
) -> Result<PipelineOutcome<CodegenReport>, String> {
    let mut diagnostics = project.schema_diagnostic_set();
    diagnostics.extend(project.codegen_diagnostic_set());
    if !diagnostics.is_empty() {
        return Ok(PipelineOutcome::Diagnostics(diagnostics));
    }
    let command = format!("coflow codegen {codegen_id}");
    let output = required_code_output(project, codegen_id, &command)?;
    let data_format = configured_data_format(project, &command)?;
    let Some(codegen) = registry.codegen(codegen_id) else {
        return Ok(PipelineOutcome::Diagnostics(project_diagnostic_set(
            &project.config_path,
            format!("no code generator registered for `{codegen_id}`"),
            ["outputs", "code", "type"],
        )));
    };
    let codegen_descriptor = codegen.descriptor();
    if !codegen_descriptor
        .supported_data_formats
        .contains(&data_format)
    {
        return Ok(PipelineOutcome::Diagnostics(project_diagnostic_set(
            &project.config_path,
            format!("code generator `{codegen_id}` does not support data format `{data_format}`"),
            ["outputs", "code", "type"],
        )));
    }
    let dir = output_dir(project, output, options.out_dir);
    let schema = match compile_project_schema(project)? {
        Ok(schema) => schema,
        Err(diagnostics) => return Ok(PipelineOutcome::Diagnostics(diagnostics)),
    };
    let codegen_diagnostics =
        preflight_codegen(registry, &schema, None, codegen_id, data_format, output)?;
    if !codegen_diagnostics.is_empty() {
        return Ok(PipelineOutcome::Diagnostics(codegen_diagnostics));
    }
    let artifact_diagnostics = artifact_safety_diagnostics(
        project,
        &[ArtifactOutputPlan::new("outputs.code.dir", dir.clone())],
    );
    if !artifact_diagnostics.is_empty() {
        return Ok(PipelineOutcome::Diagnostics(artifact_diagnostics));
    }
    let lockfile = enum_lockfile_path(project);
    let key_as_enum_ids = collect_declared_key_as_enum_ids(&schema);
    let (locked_key_as_enum, key_as_enum_variants) =
        match merge_key_as_enum_lockfile(&lockfile, key_as_enum_ids) {
            Ok(lockfile) => lockfile,
            Err(diagnostics) => return Ok(PipelineOutcome::Diagnostics(diagnostics)),
        };
    let key_as_enum_variants = match serde_json::to_value(key_as_enum_variants) {
        Ok(value) => value,
        Err(err) => {
            return Ok(PipelineOutcome::Diagnostics(artifact_diagnostic_set(
                &lockfile,
                format!("failed to serialize @keyAsEnum variants: {err}"),
            )))
        }
    };
    let staged_code = match stage_codegen_artifacts(
        registry,
        CodegenArtifactRequest {
            schema: &schema,
            model: None,
            codegen_id,
            data_format,
            output_config: output,
            dir: &dir,
            key_as_enum_variants: &key_as_enum_variants,
        },
    ) {
        Ok(staged_code) => staged_code,
        Err(diagnostics) => return Ok(PipelineOutcome::Diagnostics(diagnostics)),
    };
    let staged_lockfile = match stage_key_as_enum_lockfile_if_needed(&lockfile, &locked_key_as_enum)
    {
        Ok(staged_lockfile) => staged_lockfile,
        Err(diagnostics) => return Ok(PipelineOutcome::Diagnostics(diagnostics)),
    };
    if let Err(diagnostics) = commit_staged_dir_and_file(staged_code, staged_lockfile) {
        return Ok(PipelineOutcome::Diagnostics(diagnostics));
    }
    Ok(PipelineOutcome::Success(CodegenReport {
        codegen_id: codegen_id.to_string(),
        display_name: codegen_descriptor.display_name.to_string(),
        dir,
    }))
}

#[derive(Debug)]
struct BuildProviderPlan<'a> {
    data: BuildDataPlan<'a>,
    code: Option<BuildCodegenPlan<'a>>,
    artifact_outputs: Vec<ArtifactOutputPlan>,
}

#[derive(Debug)]
struct BuildDataPlan<'a> {
    output: &'a coflow_project::OutputConfig,
    exporter_id: &'a str,
    display_name: &'static str,
    dir: PathBuf,
}

#[derive(Debug)]
struct BuildCodegenPlan<'a> {
    output: &'a coflow_project::OutputConfig,
    codegen_id: &'a str,
    display_name: &'static str,
    dir: PathBuf,
    needs_model_for_build: bool,
}

fn build_config_diagnostics(project: &Project) -> DiagnosticSet {
    let mut diagnostics = project.schema_diagnostic_set();
    diagnostics.extend(project.data_diagnostic_set());
    if let Err(message) = configured_data_output(project, "coflow build") {
        diagnostics.push(project_diagnostic(
            &project.config_path,
            message,
            ["outputs", "data"],
        ));
    }
    diagnostics
}

fn build_provider_plan<'a>(
    project: &'a Project,
    registry: &ProviderRegistry,
    options: BuildOptions<'a>,
) -> Result<BuildProviderPlan<'a>, DiagnosticSet> {
    let (data_output, data_format) =
        configured_data_output(project, "coflow build").map_err(|message| {
            project_diagnostic_set(&project.config_path, message, ["outputs", "data"])
        })?;
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
            output: data_output,
            exporter_id: data_format,
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
) -> Result<Option<BuildCodegenPlan<'a>>, DiagnosticSet> {
    let Some(output) = project.config.outputs.code.as_ref() else {
        return Ok(None);
    };
    let codegen_id = output.output_type.as_str();
    let codegen = registry.codegen(codegen_id).ok_or_else(|| {
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
        output,
        codegen_id,
        display_name: descriptor.display_name,
        dir,
        needs_model_for_build: descriptor.needs_model_for_build,
    }))
}

fn build_codegen_preflight_diagnostics(
    registry: &ProviderRegistry,
    schema: &coflow_cft::CftContainer,
    model: &coflow_data_model::CfdDataModel,
    plan: &BuildProviderPlan<'_>,
) -> Result<DiagnosticSet, String> {
    let Some(code) = plan.code.as_ref() else {
        return Ok(DiagnosticSet::empty());
    };
    preflight_codegen(
        registry,
        schema,
        code.needs_model_for_build.then_some(model),
        code.codegen_id,
        plan.data.exporter_id,
        code.output,
    )
}

fn commit_build_artifacts(
    project: &Project,
    registry: &ProviderRegistry,
    schema: &coflow_cft::CftContainer,
    model: &coflow_data_model::CfdDataModel,
    staged_data: artifacts::StagedArtifactDir,
    plan: &BuildProviderPlan<'_>,
) -> Result<Option<CodegenReport>, DiagnosticSet> {
    let Some(code) = plan.code.as_ref() else {
        staged_data.commit()?;
        return Ok(None);
    };

    let lockfile = enum_lockfile_path(project);
    let key_as_enum_ids = collect_key_as_enum_ids(schema, model);
    let (locked_key_as_enum, key_as_enum_variants) =
        merge_key_as_enum_lockfile(&lockfile, key_as_enum_ids)?;
    let key_as_enum_variants = serde_json::to_value(key_as_enum_variants).map_err(|err| {
        artifact_diagnostic_set(
            &lockfile,
            format!("failed to serialize @keyAsEnum variants: {err}"),
        )
    })?;
    let staged_code = stage_codegen_artifacts(
        registry,
        CodegenArtifactRequest {
            schema,
            model: code.needs_model_for_build.then_some(model),
            codegen_id: code.codegen_id,
            data_format: plan.data.exporter_id,
            output_config: code.output,
            dir: &code.dir,
            key_as_enum_variants: &key_as_enum_variants,
        },
    )?;
    let staged_lockfile = stage_key_as_enum_lockfile_if_needed(&lockfile, &locked_key_as_enum)?;
    commit_staged_dirs_and_file(vec![staged_data, staged_code], staged_lockfile)?;
    Ok(Some(CodegenReport {
        codegen_id: code.codegen_id.to_string(),
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

#[derive(Debug)]
struct ArtifactOutputPlan {
    label: &'static str,
    dir: PathBuf,
}

impl ArtifactOutputPlan {
    const fn new(label: &'static str, dir: PathBuf) -> Self {
        Self { label, dir }
    }
}

fn artifact_safety_diagnostics(project: &Project, outputs: &[ArtifactOutputPlan]) -> DiagnosticSet {
    let mut diagnostics = DiagnosticSet::empty();
    for output in outputs {
        if output.dir.exists() && !output.dir.is_dir() {
            diagnostics.push(artifact_diagnostic(
                &output.dir,
                format!(
                    "output dir `{}` already exists and is not a directory",
                    output.dir.display()
                ),
            ));
        }
        diagnostics.extend(output_scope_diagnostics(project, output));
    }
    diagnostics.extend(overlapping_output_diagnostics(outputs));
    diagnostics
}

fn output_scope_diagnostics(project: &Project, output: &ArtifactOutputPlan) -> DiagnosticSet {
    let output_dir = normalized_existing_or_future_path(&output.dir);
    let project_root = normalized_existing_or_future_path(&project.root_dir);
    let mut diagnostics = DiagnosticSet::empty();

    if output_dir == project_root {
        diagnostics.push(artifact_diagnostic(
            &output.dir,
            format!(
                "{} `{}` overlaps the project root; choose a dedicated generated output directory",
                output.label,
                output.dir.display()
            ),
        ));
    }

    let config_path = normalized_existing_or_future_path(&project.config_path);
    if paths_overlap(&output_dir, &config_path) {
        diagnostics.push(artifact_diagnostic(
            &output.dir,
            format!(
                "{} `{}` overlaps project config `{}`",
                output.label,
                output.dir.display(),
                project.config_path.display()
            ),
        ));
    }

    for schema_path in configured_schema_paths(project) {
        let schema_path = normalized_existing_or_future_path(&schema_path);
        if paths_overlap(&output_dir, &schema_path) {
            diagnostics.push(artifact_diagnostic(
                &output.dir,
                format!(
                    "{} `{}` overlaps schema path `{}`",
                    output.label,
                    output.dir.display(),
                    schema_path.display()
                ),
            ));
        }
    }

    for source_path in configured_source_paths(project) {
        let source_path = normalized_existing_or_future_path(&source_path);
        if paths_overlap(&output_dir, &source_path) {
            diagnostics.push(artifact_diagnostic(
                &output.dir,
                format!(
                    "{} `{}` overlaps data source `{}`",
                    output.label,
                    output.dir.display(),
                    source_path.display()
                ),
            ));
        }
    }

    diagnostics
}

fn overlapping_output_diagnostics(outputs: &[ArtifactOutputPlan]) -> DiagnosticSet {
    let mut diagnostics = DiagnosticSet::empty();
    for (index, left) in outputs.iter().enumerate() {
        let left_dir = normalized_existing_or_future_path(&left.dir);
        for right in outputs.iter().skip(index + 1) {
            let right_dir = normalized_existing_or_future_path(&right.dir);
            if paths_overlap(&left_dir, &right_dir) {
                diagnostics.push(artifact_diagnostic(
                    &left.dir,
                    format!(
                        "{} `{}` and {} `{}` overlap; choose separate generated output directories",
                        left.label,
                        left.dir.display(),
                        right.label,
                        right.dir.display()
                    ),
                ));
            }
        }
    }
    diagnostics
}

fn configured_schema_paths(project: &Project) -> Vec<PathBuf> {
    match &project.config.schema {
        coflow_project::SchemaConfig::One(path) => vec![project.resolve_path(path)],
        coflow_project::SchemaConfig::Many(paths) => paths
            .iter()
            .map(|path| project.resolve_path(path))
            .collect(),
    }
}

fn configured_source_paths(project: &Project) -> Vec<PathBuf> {
    project
        .config
        .sources
        .iter()
        .filter_map(|source| match source.location() {
            SourceLocationSpec::Path(path) => Some(path),
            SourceLocationSpec::Uri(_) => None,
        })
        .map(|path| project.resolve_path(path))
        .collect()
}

fn normalized_existing_or_future_path(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| normalize_path_lexically(path))
}

fn normalize_path_lexically(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            _ => out.push(component.as_os_str()),
        }
    }
    out
}

fn paths_overlap(left: &Path, right: &Path) -> bool {
    left == right || left.starts_with(right) || right.starts_with(left)
}

fn artifact_diagnostic(path: &Path, message: impl Into<String>) -> Diagnostic {
    Diagnostic {
        code: "ARTIFACT-001".to_string(),
        stage: "ARTIFACT".to_string(),
        severity: Severity::Error,
        message: message.into(),
        primary: Some(Label {
            location: SourceLocation::Artifact {
                path: path.to_path_buf(),
            },
            message: None,
        }),
        related: Vec::new(),
    }
}

fn artifact_diagnostic_set(path: &Path, message: impl Into<String>) -> DiagnosticSet {
    DiagnosticSet::one(artifact_diagnostic(path, message))
}

fn collect_declared_key_as_enum_ids(
    schema: &coflow_cft::CftContainer,
) -> BTreeMap<String, KeyAsEnumIds> {
    let mut out = BTreeMap::new();
    for schema_type in schema.all_types() {
        if let Some(enum_name) = annotation_name_arg(&schema_type.annotations, "keyAsEnum") {
            let is_flags = schema
                .resolve_enum(&enum_name)
                .is_some_and(|schema_enum| has_annotation(&schema_enum.annotations, "flag"));
            out.entry(enum_name).or_insert_with(|| KeyAsEnumIds {
                ids: Vec::new(),
                is_flags,
            });
        }
    }
    out
}

fn collect_key_as_enum_ids(
    schema: &coflow_cft::CftContainer,
    model: &coflow_data_model::CfdDataModel,
) -> BTreeMap<String, KeyAsEnumIds> {
    let mut out = collect_declared_key_as_enum_ids(schema);
    for schema_type in schema.all_types() {
        let Some(enum_name) = annotation_name_arg(&schema_type.annotations, "keyAsEnum") else {
            continue;
        };

        let mut seen = BTreeSet::new();
        let mut variants = Vec::new();
        if let Some(index) = model.polymorphic_index(&schema_type.name) {
            for key in index.records.keys() {
                if seen.insert(key.clone()) {
                    variants.push(key.clone());
                }
            }
        } else {
            for (_record_id, record) in model.records_of_type(&schema_type.name) {
                let key = record.key();
                if seen.insert(key.to_string()) {
                    variants.push(key.to_string());
                }
            }
        }
        if let Some(entry) = out.get_mut(&enum_name) {
            entry.ids = variants;
        }
    }
    out
}

type KeyAsEnumLockfile = BTreeMap<String, BTreeMap<String, i64>>;

#[derive(Debug, Clone)]
struct KeyAsEnumIds {
    ids: Vec<String>,
    is_flags: bool,
}

fn enum_lockfile_path(project: &Project) -> PathBuf {
    project
        .config_path
        .parent()
        .unwrap_or(&project.root_dir)
        .join(ENUM_LOCKFILE_NAME)
}

fn merge_key_as_enum_lockfile(
    lockfile: &Path,
    current_ids: BTreeMap<String, KeyAsEnumIds>,
) -> Result<(KeyAsEnumLockfile, BTreeMap<String, Vec<KeyAsEnumVariant>>), DiagnosticSet> {
    if current_ids.is_empty() {
        return Ok((BTreeMap::new(), BTreeMap::new()));
    }

    let mut locked = read_key_as_enum_lockfile(lockfile)?;
    locked.retain(|enum_name, _| current_ids.contains_key(enum_name));

    for (enum_name, key_enum) in current_ids {
        let entries = locked.entry(enum_name).or_default();
        validate_existing_key_as_enum_values(lockfile, entries, key_enum.is_flags)?;
        let mut next_value = next_key_as_enum_value_start(lockfile, entries, key_enum.is_flags)?;
        for id in key_enum.ids {
            if entries.contains_key(&id) {
                continue;
            }
            while entries.values().any(|value| *value == next_value) {
                next_value = next_key_as_enum_value(lockfile, next_value, key_enum.is_flags)?;
            }
            entries.insert(id, next_value);
            next_value = next_key_as_enum_value(lockfile, next_value, key_enum.is_flags)?;
        }
    }

    let variants = locked
        .into_iter()
        .map(|(enum_name, entries)| {
            let mut variants = entries
                .into_iter()
                .map(|(name, value)| KeyAsEnumVariant { name, value })
                .collect::<Vec<_>>();
            variants.sort_by(|left, right| {
                left.value
                    .cmp(&right.value)
                    .then_with(|| left.name.cmp(&right.name))
            });
            (enum_name, variants)
        })
        .collect();

    let locked = variants_to_lockfile(&variants);
    Ok((locked, variants))
}

fn next_key_as_enum_value_start(
    lockfile: &Path,
    entries: &BTreeMap<String, i64>,
    is_flags: bool,
) -> Result<i64, DiagnosticSet> {
    let Some(value) = entries.values().copied().max() else {
        return Ok(if is_flags { 1 } else { 0 });
    };
    next_key_as_enum_value(lockfile, value, is_flags)
}

fn next_key_as_enum_value(
    lockfile: &Path,
    value: i64,
    is_flags: bool,
) -> Result<i64, DiagnosticSet> {
    if is_flags {
        return value.checked_mul(2).ok_or_else(|| {
            artifact_diagnostic_set(
                lockfile,
                "@keyAsEnum lockfile exhausted i64 flag enum values",
            )
        });
    }
    value.checked_add(1).ok_or_else(|| {
        artifact_diagnostic_set(lockfile, "@keyAsEnum lockfile exhausted i64 enum values")
    })
}

fn validate_existing_key_as_enum_values(
    lockfile: &Path,
    entries: &BTreeMap<String, i64>,
    is_flags: bool,
) -> Result<(), DiagnosticSet> {
    if !is_flags {
        return Ok(());
    }
    if let Some((name, value)) = entries
        .iter()
        .find(|(_, value)| **value <= 0 || (**value & (**value - 1)) != 0)
    {
        return Err(artifact_diagnostic_set(
            lockfile,
            format!("@keyAsEnum flag enum variant `{name}` has non-flag lockfile value `{value}`"),
        ));
    }
    Ok(())
}

fn read_key_as_enum_lockfile(
    path: &Path,
) -> Result<BTreeMap<String, BTreeMap<String, i64>>, DiagnosticSet> {
    if !path.exists() {
        return Ok(BTreeMap::new());
    }

    let contents = fs::read_to_string(path).map_err(|err| {
        artifact_diagnostic_set(path, format!("failed to read `{}`: {err}", path.display()))
    })?;
    serde_json::from_str(&contents).map_err(|err| {
        artifact_diagnostic_set(path, format!("failed to parse `{}`: {err}", path.display()))
    })
}

fn stage_key_as_enum_lockfile_if_needed(
    path: &Path,
    locked: &KeyAsEnumLockfile,
) -> Result<Option<artifacts::StagedArtifactFile>, DiagnosticSet> {
    if locked.is_empty() {
        return Ok(None);
    }
    stage_json_file(path, locked).map(Some)
}

fn variants_to_lockfile(variants: &BTreeMap<String, Vec<KeyAsEnumVariant>>) -> KeyAsEnumLockfile {
    variants
        .iter()
        .map(|(enum_name, entries)| {
            (
                enum_name.clone(),
                entries
                    .iter()
                    .map(|entry| (entry.name.clone(), entry.value))
                    .collect(),
            )
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct KeyAsEnumVariant {
    name: String,
    value: i64,
}

fn annotation_name_arg(annotations: &[coflow_cft::CftAnnotation], name: &str) -> Option<String> {
    annotations
        .iter()
        .find(|annotation| annotation.name == name)
        .and_then(|annotation| annotation.args.first())
        .and_then(|arg| match arg {
            coflow_cft::CftAnnotationValue::Name(value) => Some(value.clone()),
            _ => None,
        })
}

fn has_annotation(annotations: &[coflow_cft::CftAnnotation], name: &str) -> bool {
    annotations.iter().any(|annotation| annotation.name == name)
}
