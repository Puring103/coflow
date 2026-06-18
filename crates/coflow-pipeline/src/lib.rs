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
use coflow_api::{DiagnosticSet, ProviderRegistry};
use coflow_project::{DiagnosticJson, Project};
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
    Diagnostics(Vec<DiagnosticJson>),
}

#[derive(Debug, Clone, Copy, Default)]
pub struct BuildOptions<'a> {
    pub data_out_dir: Option<&'a Path>,
    pub code_out_dir: Option<&'a Path>,
    pub namespace: Option<&'a str>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ExportOptions<'a> {
    pub out_dir: Option<&'a Path>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct CodegenOptions<'a> {
    pub out_dir: Option<&'a Path>,
    pub namespace: Option<&'a str>,
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
    let mut diagnostics = project.schema_diagnostics();
    diagnostics.extend(project.data_diagnostics());
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

    let staged_data = stage_data_tables(
        registry,
        &schema,
        &load_output.model,
        plan.data.exporter_id,
        plan.data.output,
        &plan.data.dir,
    )?;
    let code = commit_build_artifacts(
        project,
        registry,
        &schema,
        &load_output.model,
        staged_data,
        &plan,
    )?;

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
    let mut diagnostics = project.schema_diagnostics();
    diagnostics.extend(project.data_diagnostics());
    let command = format!("coflow export {exporter_id}");
    if let Err(message) = required_data_output(project, exporter_id, &command) {
        diagnostics.push(DiagnosticJson::project(message));
    }
    if !diagnostics.is_empty() {
        return Ok(PipelineOutcome::Diagnostics(diagnostics));
    }
    let Some(exporter) = registry.exporter(exporter_id) else {
        return Ok(PipelineOutcome::Diagnostics(vec![DiagnosticJson::project(
            format!("no data exporter registered for `{exporter_id}`"),
        )]));
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
    write_data_tables(
        registry,
        &schema,
        &load_output.model,
        exporter_id,
        output,
        &dir,
    )?;
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
    let mut diagnostics = project.schema_diagnostics();
    diagnostics.extend(project.codegen_diagnostics());
    if !diagnostics.is_empty() {
        return Ok(PipelineOutcome::Diagnostics(diagnostics));
    }
    let command = format!("coflow codegen {codegen_id}");
    let output = required_code_output(project, codegen_id, &command)?;
    let data_format = configured_data_format(project, &command)?;
    let Some(codegen) = registry.codegen(codegen_id) else {
        return Ok(PipelineOutcome::Diagnostics(vec![DiagnosticJson::project(
            format!("no code generator registered for `{codegen_id}`"),
        )]));
    };
    let codegen_descriptor = codegen.descriptor();
    if !codegen_descriptor
        .supported_data_formats
        .contains(&data_format)
    {
        return Ok(PipelineOutcome::Diagnostics(vec![DiagnosticJson::project(
            format!("code generator `{codegen_id}` does not support data format `{data_format}`"),
        )]));
    }
    let dir = output_dir(project, output, options.out_dir);
    let namespace = options
        .namespace
        .or(output.namespace.as_deref())
        .unwrap_or("Game.Config");
    let schema = match compile_project_schema(project)? {
        Ok(schema) => schema,
        Err(diagnostics) => return Ok(PipelineOutcome::Diagnostics(diagnostics)),
    };
    let codegen_diagnostics = diagnostics_from_provider(preflight_codegen(
        registry,
        &schema,
        None,
        codegen_id,
        data_format,
        output,
        namespace,
    )?);
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
        merge_key_as_enum_lockfile(&lockfile, key_as_enum_ids)?;
    let key_as_enum_variants = serde_json::to_value(key_as_enum_variants)
        .map_err(|err| format!("failed to serialize @keyAsEnum variants: {err}"))?;
    let staged_code = stage_codegen_artifacts(
        registry,
        CodegenArtifactRequest {
            schema: &schema,
            model: None,
            codegen_id,
            data_format,
            output_config: output,
            namespace,
            dir: &dir,
            key_as_enum_variants: &key_as_enum_variants,
        },
    )?;
    let staged_lockfile = stage_key_as_enum_lockfile_if_needed(&lockfile, &locked_key_as_enum)?;
    commit_staged_dir_and_file(staged_code, staged_lockfile)?;
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
    namespace: String,
    needs_model_for_build: bool,
}

fn build_config_diagnostics(project: &Project) -> Vec<DiagnosticJson> {
    let mut diagnostics = project.schema_diagnostics();
    diagnostics.extend(project.data_diagnostics());
    if let Err(message) = configured_data_output(project, "coflow build") {
        diagnostics.push(DiagnosticJson::project(message));
    }
    diagnostics
}

fn build_provider_plan<'a>(
    project: &'a Project,
    registry: &ProviderRegistry,
    options: BuildOptions<'a>,
) -> Result<BuildProviderPlan<'a>, Vec<DiagnosticJson>> {
    let (data_output, data_format) =
        configured_data_output(project, "coflow build").map_err(project_diagnostic_vec)?;
    let data_exporter = registry.exporter(data_format).ok_or_else(|| {
        project_diagnostic_vec(format!("no data exporter registered for `{data_format}`"))
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
) -> Result<Option<BuildCodegenPlan<'a>>, Vec<DiagnosticJson>> {
    let Some(output) = project.config.outputs.code.as_ref() else {
        return Ok(None);
    };
    let codegen_id = output.output_type.as_str();
    let codegen = registry.codegen(codegen_id).ok_or_else(|| {
        project_diagnostic_vec(format!("no code generator registered for `{codegen_id}`"))
    })?;
    let descriptor = codegen.descriptor();
    if !descriptor.supported_data_formats.contains(&data_format) {
        return Err(project_diagnostic_vec(format!(
            "code generator `{codegen_id}` does not support data format `{data_format}`"
        )));
    }

    let dir = output_dir(project, output, options.code_out_dir);
    artifact_outputs.push(ArtifactOutputPlan::new("outputs.code.dir", dir.clone()));
    Ok(Some(BuildCodegenPlan {
        output,
        codegen_id,
        display_name: descriptor.display_name,
        dir,
        namespace: options
            .namespace
            .or(output.namespace.as_deref())
            .unwrap_or("Game.Config")
            .to_string(),
        needs_model_for_build: descriptor.needs_model_for_build,
    }))
}

fn build_codegen_preflight_diagnostics(
    registry: &ProviderRegistry,
    schema: &coflow_cft::CftContainer,
    model: &coflow_data_model::CfdDataModel,
    plan: &BuildProviderPlan<'_>,
) -> Result<Vec<DiagnosticJson>, String> {
    let Some(code) = plan.code.as_ref() else {
        return Ok(Vec::new());
    };
    Ok(diagnostics_from_provider(preflight_codegen(
        registry,
        schema,
        code.needs_model_for_build.then_some(model),
        code.codegen_id,
        plan.data.exporter_id,
        code.output,
        &code.namespace,
    )?))
}

fn commit_build_artifacts(
    project: &Project,
    registry: &ProviderRegistry,
    schema: &coflow_cft::CftContainer,
    model: &coflow_data_model::CfdDataModel,
    staged_data: artifacts::StagedArtifactDir,
    plan: &BuildProviderPlan<'_>,
) -> Result<Option<CodegenReport>, String> {
    let Some(code) = plan.code.as_ref() else {
        staged_data.commit()?;
        return Ok(None);
    };

    let lockfile = enum_lockfile_path(project);
    let key_as_enum_ids = collect_key_as_enum_ids(schema, model);
    let (locked_key_as_enum, key_as_enum_variants) =
        merge_key_as_enum_lockfile(&lockfile, key_as_enum_ids)?;
    let key_as_enum_variants = serde_json::to_value(key_as_enum_variants)
        .map_err(|err| format!("failed to serialize @keyAsEnum variants: {err}"))?;
    let staged_code = stage_codegen_artifacts(
        registry,
        CodegenArtifactRequest {
            schema,
            model: code.needs_model_for_build.then_some(model),
            codegen_id: code.codegen_id,
            data_format: plan.data.exporter_id,
            output_config: code.output,
            namespace: &code.namespace,
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

fn project_diagnostic_vec(message: String) -> Vec<DiagnosticJson> {
    vec![DiagnosticJson::project(message)]
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

fn artifact_safety_diagnostics(
    project: &Project,
    outputs: &[ArtifactOutputPlan],
) -> Vec<DiagnosticJson> {
    let mut diagnostics = Vec::new();
    for output in outputs {
        if output.dir.exists() && !output.dir.is_dir() {
            diagnostics.push(DiagnosticJson::artifact(format!(
                "output dir `{}` already exists and is not a directory",
                output.dir.display()
            )));
        }
        diagnostics.extend(output_scope_diagnostics(project, output));
    }
    diagnostics.extend(overlapping_output_diagnostics(outputs));
    diagnostics
}

fn output_scope_diagnostics(project: &Project, output: &ArtifactOutputPlan) -> Vec<DiagnosticJson> {
    let output_dir = normalized_existing_or_future_path(&output.dir);
    let project_root = normalized_existing_or_future_path(&project.root_dir);
    let mut diagnostics = Vec::new();

    if output_dir == project_root {
        diagnostics.push(DiagnosticJson::artifact(format!(
            "{} `{}` overlaps the project root; choose a dedicated generated output directory",
            output.label,
            output.dir.display()
        )));
    }

    let config_path = normalized_existing_or_future_path(&project.config_path);
    if paths_overlap(&output_dir, &config_path) {
        diagnostics.push(DiagnosticJson::artifact(format!(
            "{} `{}` overlaps project config `{}`",
            output.label,
            output.dir.display(),
            project.config_path.display()
        )));
    }

    for schema_path in configured_schema_paths(project) {
        let schema_path = normalized_existing_or_future_path(&schema_path);
        if paths_overlap(&output_dir, &schema_path) {
            diagnostics.push(DiagnosticJson::artifact(format!(
                "{} `{}` overlaps schema path `{}`",
                output.label,
                output.dir.display(),
                schema_path.display()
            )));
        }
    }

    for source_path in configured_source_paths(project) {
        let source_path = normalized_existing_or_future_path(&source_path);
        if paths_overlap(&output_dir, &source_path) {
            diagnostics.push(DiagnosticJson::artifact(format!(
                "{} `{}` overlaps data source `{}`",
                output.label,
                output.dir.display(),
                source_path.display()
            )));
        }
    }

    diagnostics
}

fn overlapping_output_diagnostics(outputs: &[ArtifactOutputPlan]) -> Vec<DiagnosticJson> {
    let mut diagnostics = Vec::new();
    for (index, left) in outputs.iter().enumerate() {
        let left_dir = normalized_existing_or_future_path(&left.dir);
        for right in outputs.iter().skip(index + 1) {
            let right_dir = normalized_existing_or_future_path(&right.dir);
            if paths_overlap(&left_dir, &right_dir) {
                diagnostics.push(DiagnosticJson::artifact(format!(
                    "{} `{}` and {} `{}` overlap; choose separate generated output directories",
                    left.label,
                    left.dir.display(),
                    right.label,
                    right.dir.display()
                )));
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
        .filter_map(|source| source.file.as_ref().or(source.dir.as_ref()))
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

fn diagnostics_from_provider(diagnostics: DiagnosticSet) -> Vec<DiagnosticJson> {
    diagnostics
        .diagnostics
        .into_iter()
        .map(|diagnostic| {
            DiagnosticJson::codegen(diagnostic.code, diagnostic.stage, diagnostic.message)
        })
        .collect()
}

fn collect_declared_key_as_enum_ids(
    schema: &coflow_cft::CftContainer,
) -> BTreeMap<String, Vec<String>> {
    let mut out = BTreeMap::new();
    for schema_type in schema.all_types() {
        if let Some(enum_name) = annotation_string_arg(&schema_type.annotations, "keyAsEnum") {
            out.entry(enum_name).or_default();
        }
    }
    out
}

fn collect_key_as_enum_ids(
    schema: &coflow_cft::CftContainer,
    model: &coflow_data_model::CfdDataModel,
) -> BTreeMap<String, Vec<String>> {
    let mut out = collect_declared_key_as_enum_ids(schema);
    for schema_type in schema.all_types() {
        let Some(enum_name) = annotation_string_arg(&schema_type.annotations, "keyAsEnum") else {
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
        out.insert(enum_name, variants);
    }
    out
}

type KeyAsEnumLockfile = BTreeMap<String, BTreeMap<String, i64>>;

fn enum_lockfile_path(project: &Project) -> PathBuf {
    project
        .config_path
        .parent()
        .unwrap_or(&project.root_dir)
        .join(ENUM_LOCKFILE_NAME)
}

fn merge_key_as_enum_lockfile(
    lockfile: &Path,
    current_ids: BTreeMap<String, Vec<String>>,
) -> Result<(KeyAsEnumLockfile, BTreeMap<String, Vec<KeyAsEnumVariant>>), String> {
    if current_ids.is_empty() {
        return Ok((BTreeMap::new(), BTreeMap::new()));
    }

    let mut locked = read_key_as_enum_lockfile(lockfile)?;
    locked.retain(|enum_name, _| current_ids.contains_key(enum_name));

    for (enum_name, ids) in current_ids {
        let entries = locked.entry(enum_name).or_default();
        let mut next_value = entries
            .values()
            .copied()
            .max()
            .map_or(Ok(0), next_key_as_enum_value)?;
        for id in ids {
            if entries.contains_key(&id) {
                continue;
            }
            while entries.values().any(|value| *value == next_value) {
                next_value = next_key_as_enum_value(next_value)?;
            }
            entries.insert(id, next_value);
            next_value = next_key_as_enum_value(next_value)?;
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

fn next_key_as_enum_value(value: i64) -> Result<i64, String> {
    value
        .checked_add(1)
        .ok_or_else(|| "@keyAsEnum lockfile exhausted i64 enum values".to_string())
}

fn read_key_as_enum_lockfile(
    path: &Path,
) -> Result<BTreeMap<String, BTreeMap<String, i64>>, String> {
    if !path.exists() {
        return Ok(BTreeMap::new());
    }

    let contents = fs::read_to_string(path)
        .map_err(|err| format!("failed to read `{}`: {err}", path.display()))?;
    serde_json::from_str(&contents)
        .map_err(|err| format!("failed to parse `{}`: {err}", path.display()))
}

fn stage_key_as_enum_lockfile_if_needed(
    path: &Path,
    locked: &KeyAsEnumLockfile,
) -> Result<Option<artifacts::StagedArtifactFile>, String> {
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

fn annotation_string_arg(annotations: &[coflow_cft::CftAnnotation], name: &str) -> Option<String> {
    annotations
        .iter()
        .find(|annotation| annotation.name == name)
        .and_then(|annotation| annotation.args.first())
        .and_then(|arg| match arg {
            coflow_cft::CftAnnotationValue::String(value) => Some(value.clone()),
            _ => None,
        })
}
