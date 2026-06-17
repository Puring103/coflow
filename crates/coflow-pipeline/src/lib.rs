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

mod artifacts;
mod data;
mod schema;

use artifacts::{
    commit_staged_dir_and_file, commit_staged_dirs_and_file, configured_data_format,
    configured_data_output, output_dir, preflight_csharp_files, required_code_output,
    required_data_output, stage_csharp_files, stage_data_tables, stage_json_file,
    write_data_tables,
};
use coflow_codegen_csharp_json::CsharpCodegenDiagnostic;
use coflow_codegen_csharp_json::CsharpKeyAsEnumVariant;
use coflow_project::{DiagnosticJson, Project};
use data::load_project_data;
use schema::compile_project_schema;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

const ENUM_LOCKFILE_NAME: &str = "coflow.enum.lock.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataFormat {
    Json,
    Messagepack,
}

impl DataFormat {
    #[must_use]
    pub const fn as_config_value(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Messagepack => "messagepack",
        }
    }

    #[must_use]
    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Json => "JSON",
            Self::Messagepack => "MessagePack",
        }
    }

    #[must_use]
    pub fn from_config_value(value: &str) -> Option<Self> {
        match value {
            "json" => Some(Self::Json),
            "messagepack" => Some(Self::Messagepack),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodegenTarget {
    Csharp,
}

impl CodegenTarget {
    #[must_use]
    pub const fn as_config_value(self) -> &'static str {
        match self {
            Self::Csharp => "csharp",
        }
    }

    #[must_use]
    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Csharp => "C#",
        }
    }
}

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
    pub format: DataFormat,
    pub dir: PathBuf,
}

#[derive(Debug)]
pub struct CodegenReport {
    pub target: CodegenTarget,
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
pub fn check_project(project: &Project) -> Result<PipelineOutcome<CheckReport>, String> {
    let mut diagnostics = project.schema_diagnostics();
    diagnostics.extend(project.data_diagnostics());
    if !diagnostics.is_empty() {
        return Ok(PipelineOutcome::Diagnostics(diagnostics));
    }
    let schema = match compile_project_schema(project)? {
        Ok(schema) => schema,
        Err(diagnostics) => return Ok(PipelineOutcome::Diagnostics(diagnostics)),
    };
    match load_project_data(project, &schema) {
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
    options: BuildOptions<'_>,
) -> Result<PipelineOutcome<BuildReport>, String> {
    let mut diagnostics = project.schema_diagnostics();
    diagnostics.extend(project.data_diagnostics());
    if let Err(message) = configured_data_output(project, "coflow build") {
        diagnostics.push(DiagnosticJson::project(message));
    }
    if !diagnostics.is_empty() {
        return Ok(PipelineOutcome::Diagnostics(diagnostics));
    }
    let (data_output, data_format) = configured_data_output(project, "coflow build")?;
    let schema = match compile_project_schema(project)? {
        Ok(schema) => schema,
        Err(diagnostics) => return Ok(PipelineOutcome::Diagnostics(diagnostics)),
    };
    let load_output = match load_project_data(project, &schema) {
        Ok(output) => output,
        Err(diagnostics) => return Ok(PipelineOutcome::Diagnostics(diagnostics)),
    };

    let data_dir = output_dir(project, data_output, options.data_out_dir);
    let mut artifact_plans = vec![ArtifactOutputPlan::new(
        "outputs.data.dir",
        data_dir.clone(),
    )];
    let mut preflight_diagnostics = Vec::new();
    let code_plan = if let Some(code_output) = project.config.outputs.code.as_ref() {
        if code_output.output_type != CodegenTarget::Csharp.as_config_value() {
            return Err(format!(
                "coflow.yaml outputs.code.type is `{}`; expected `csharp`",
                code_output.output_type
            ));
        }
        let code_dir = output_dir(project, code_output, options.code_out_dir);
        let namespace = options
            .namespace
            .or(code_output.namespace.as_deref())
            .unwrap_or("Game.Config")
            .to_string();
        preflight_diagnostics.extend(diagnostics_from_codegen_preflight(preflight_csharp_files(
            &schema, &namespace,
        )));
        artifact_plans.push(ArtifactOutputPlan::new(
            "outputs.code.dir",
            code_dir.clone(),
        ));
        Some((code_dir, namespace))
    } else {
        None
    };
    preflight_diagnostics.extend(artifact_safety_diagnostics(project, &artifact_plans));
    if !preflight_diagnostics.is_empty() {
        return Ok(PipelineOutcome::Diagnostics(preflight_diagnostics));
    }

    let staged_data = stage_data_tables(&schema, &load_output.model, data_format, &data_dir)?;
    let code = if let Some((code_dir, namespace)) = code_plan {
        let lockfile = enum_lockfile_path(project);
        let key_as_enum_ids = collect_key_as_enum_ids(&schema, &load_output.model);
        let (locked_key_as_enum, key_as_enum_variants) =
            merge_key_as_enum_lockfile(&lockfile, key_as_enum_ids)?;
        let staged_code = stage_csharp_files(
            &schema,
            data_format,
            &namespace,
            &code_dir,
            key_as_enum_variants,
        )?;
        let staged_lockfile = stage_key_as_enum_lockfile_if_needed(&lockfile, &locked_key_as_enum)?;
        commit_staged_dirs_and_file(vec![staged_data, staged_code], staged_lockfile)?;
        Some(CodegenReport {
            target: CodegenTarget::Csharp,
            dir: code_dir,
        })
    } else {
        staged_data.commit()?;
        None
    };

    let data = ExportReport {
        format: data_format,
        dir: data_dir,
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
    format: DataFormat,
    options: ExportOptions<'_>,
) -> Result<PipelineOutcome<ExportReport>, String> {
    let mut diagnostics = project.schema_diagnostics();
    diagnostics.extend(project.data_diagnostics());
    let command = format!("coflow export {}", format.as_config_value());
    if let Err(message) = required_data_output(project, format, &command) {
        diagnostics.push(DiagnosticJson::project(message));
    }
    if !diagnostics.is_empty() {
        return Ok(PipelineOutcome::Diagnostics(diagnostics));
    }
    let output = required_data_output(project, format, &command)?;
    let dir = output_dir(project, output, options.out_dir);
    let schema = match compile_project_schema(project)? {
        Ok(schema) => schema,
        Err(diagnostics) => return Ok(PipelineOutcome::Diagnostics(diagnostics)),
    };
    let load_output = match load_project_data(project, &schema) {
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
    write_data_tables(&schema, &load_output.model, format, &dir)?;
    Ok(PipelineOutcome::Success(ExportReport { format, dir }))
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
    target: CodegenTarget,
    options: CodegenOptions<'_>,
) -> Result<PipelineOutcome<CodegenReport>, String> {
    let mut diagnostics = project.schema_diagnostics();
    diagnostics.extend(project.codegen_diagnostics());
    if !diagnostics.is_empty() {
        return Ok(PipelineOutcome::Diagnostics(diagnostics));
    }
    let output = required_code_output(project, target, "coflow codegen csharp")?;
    let data_format = configured_data_format(project, "coflow codegen csharp")?;
    let dir = output_dir(project, output, options.out_dir);
    let namespace = options
        .namespace
        .or(output.namespace.as_deref())
        .unwrap_or("Game.Config");
    let schema = match compile_project_schema(project)? {
        Ok(schema) => schema,
        Err(diagnostics) => return Ok(PipelineOutcome::Diagnostics(diagnostics)),
    };
    let codegen_diagnostics =
        diagnostics_from_codegen_preflight(preflight_csharp_files(&schema, namespace));
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
    let staged_code =
        stage_csharp_files(&schema, data_format, namespace, &dir, key_as_enum_variants)?;
    let staged_lockfile = stage_key_as_enum_lockfile_if_needed(&lockfile, &locked_key_as_enum)?;
    commit_staged_dir_and_file(staged_code, staged_lockfile)?;
    Ok(PipelineOutcome::Success(CodegenReport { target, dir }))
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

fn diagnostics_from_codegen_preflight(
    diagnostics: Vec<CsharpCodegenDiagnostic>,
) -> Vec<DiagnosticJson> {
    diagnostics
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
) -> Result<
    (
        KeyAsEnumLockfile,
        BTreeMap<String, Vec<CsharpKeyAsEnumVariant>>,
    ),
    String,
> {
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
                .map(|(name, value)| CsharpKeyAsEnumVariant { name, value })
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

fn variants_to_lockfile(
    variants: &BTreeMap<String, Vec<CsharpKeyAsEnumVariant>>,
) -> KeyAsEnumLockfile {
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
