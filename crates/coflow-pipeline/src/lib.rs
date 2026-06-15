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
mod excel;
mod schema;

use artifacts::{
    configured_data_format, configured_data_output, output_dir, preflight_csharp_files,
    required_code_output, required_data_output, write_csharp_files, write_data_tables,
};
use coflow_codegen_csharp_json::CsharpCodegenDiagnostic;
use coflow_codegen_csharp_json::CsharpKeyAsEnumVariant;
use coflow_project::{DiagnosticJson, Project};
use excel::load_project_excel;
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
/// artifact errors. Schema, Excel, data-model, and check diagnostics are
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
    match load_project_excel(project, &schema) {
        Ok(_) => Ok(PipelineOutcome::Success(CheckReport)),
        Err(diagnostics) => Ok(PipelineOutcome::Diagnostics(diagnostics)),
    }
}

/// Runs validation, data export, and configured code generation.
///
/// # Errors
///
/// Returns an error for invalid project/output configuration, unsupported
/// output targets, or artifact write/codegen failures. Schema, Excel,
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
    let load_output = match load_project_excel(project, &schema) {
        Ok(output) => output,
        Err(diagnostics) => return Ok(PipelineOutcome::Diagnostics(diagnostics)),
    };

    let data_dir = output_dir(project, data_output, options.data_out_dir);
    let mut preflight_diagnostics = artifact_diagnostics_for_dirs([&data_dir]);
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
        preflight_diagnostics.extend(artifact_diagnostics_for_dirs([&code_dir]));
        Some((code_dir, namespace))
    } else {
        None
    };
    if !preflight_diagnostics.is_empty() {
        return Ok(PipelineOutcome::Diagnostics(preflight_diagnostics));
    }

    write_data_tables(&schema, &load_output, data_format, &data_dir)?;
    let data = ExportReport {
        format: data_format,
        dir: data_dir,
    };

    let code = if let Some((code_dir, namespace)) = code_plan {
        let key_as_enum_ids = collect_key_as_enum_ids(&schema, &load_output.model);
        let key_as_enum_variants = merge_key_as_enum_lockfile(&code_dir, key_as_enum_ids)?;
        write_csharp_files(
            &schema,
            data_format,
            &namespace,
            &code_dir,
            key_as_enum_variants,
        )?;
        Some(CodegenReport {
            target: CodegenTarget::Csharp,
            dir: code_dir,
        })
    } else {
        None
    };

    Ok(PipelineOutcome::Success(BuildReport { data, code }))
}

/// Exports project data in the requested format.
///
/// # Errors
///
/// Returns an error for invalid project/output configuration, unsupported data
/// format configuration, or artifact write failures. Schema, Excel,
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
    let load_output = match load_project_excel(project, &schema) {
        Ok(output) => output,
        Err(diagnostics) => return Ok(PipelineOutcome::Diagnostics(diagnostics)),
    };
    let artifact_diagnostics = artifact_diagnostics_for_dirs([&dir]);
    if !artifact_diagnostics.is_empty() {
        return Ok(PipelineOutcome::Diagnostics(artifact_diagnostics));
    }
    write_data_tables(&schema, &load_output, format, &dir)?;
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
    let artifact_diagnostics = artifact_diagnostics_for_dirs([&dir]);
    if !artifact_diagnostics.is_empty() {
        return Ok(PipelineOutcome::Diagnostics(artifact_diagnostics));
    }
    let key_as_enum_ids = collect_declared_key_as_enum_ids(&schema);
    let key_as_enum_variants = merge_key_as_enum_lockfile(&dir, key_as_enum_ids)?;
    write_csharp_files(&schema, data_format, namespace, &dir, key_as_enum_variants)?;
    Ok(PipelineOutcome::Success(CodegenReport { target, dir }))
}

fn artifact_diagnostics_for_dirs<'a>(
    dirs: impl IntoIterator<Item = &'a PathBuf>,
) -> Vec<DiagnosticJson> {
    dirs.into_iter()
        .filter(|dir| dir.exists() && !dir.is_dir())
        .map(|dir| {
            DiagnosticJson::artifact(format!(
                "output dir `{}` already exists and is not a directory",
                dir.display()
            ))
        })
        .collect()
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
        for field in &schema_type.all_fields {
            if !field
                .annotations
                .iter()
                .any(|annotation| annotation.name == "id")
            {
                continue;
            }
            if let Some(enum_name) = annotation_string_arg(&field.annotations, "IdAsEnum") {
                out.entry(enum_name).or_default();
            }
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
        let Some(id_field) = schema_type.all_fields.iter().find(|field| {
            field
                .annotations
                .iter()
                .any(|annotation| annotation.name == "id")
        }) else {
            continue;
        };
        let Some(enum_name) = annotation_string_arg(&id_field.annotations, "IdAsEnum") else {
            continue;
        };

        let mut seen = BTreeSet::new();
        let mut variants = Vec::new();
        for (_record_id, record) in model.records_of_type(&schema_type.name) {
            let Some(coflow_data_model::CfdValue::String(value)) =
                record.fields.get(&id_field.name)
            else {
                continue;
            };
            if seen.insert(value.clone()) {
                variants.push(value.clone());
            }
        }
        out.insert(enum_name, variants);
    }
    out
}

fn merge_key_as_enum_lockfile(
    code_dir: &Path,
    current_ids: BTreeMap<String, Vec<String>>,
) -> Result<BTreeMap<String, Vec<CsharpKeyAsEnumVariant>>, String> {
    if current_ids.is_empty() {
        return Ok(BTreeMap::new());
    }

    let lockfile = code_dir.join(ENUM_LOCKFILE_NAME);
    let mut locked = read_key_as_enum_lockfile(&lockfile)?;
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

    write_key_as_enum_lockfile(&lockfile, &locked)?;

    Ok(locked
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
        .collect())
}

fn next_key_as_enum_value(value: i64) -> Result<i64, String> {
    value
        .checked_add(1)
        .ok_or_else(|| "@IdAsEnum lockfile exhausted i64 enum values".to_string())
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

fn write_key_as_enum_lockfile(
    path: &Path,
    locked: &BTreeMap<String, BTreeMap<String, i64>>,
) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("failed to create `{}`: {err}", parent.display()))?;
    }
    let file = fs::File::create(path)
        .map_err(|err| format!("failed to create `{}`: {err}", path.display()))?;
    serde_json::to_writer_pretty(file, locked)
        .map_err(|err| format!("failed to write `{}`: {err}", path.display()))
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
