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
    configured_data_format, configured_data_output, output_dir, required_code_output,
    required_data_output, write_csharp_files, write_data_tables,
};
use coflow_project::{DiagnosticJson, Project};
use excel::load_project_excel;
use schema::compile_project_schema;
use std::path::{Path, PathBuf};

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
    let schema = match compile_project_schema(project)? {
        Ok(schema) => schema,
        Err(diagnostics) => return Ok(PipelineOutcome::Diagnostics(diagnostics)),
    };
    project.validate_for_data()?;
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
    project.validate_for_data()?;
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
    write_data_tables(&schema, &load_output, data_format, &data_dir)?;
    let data = ExportReport {
        format: data_format,
        dir: data_dir,
    };

    let code = if let Some(code_output) = project.config.outputs.code.as_ref() {
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
            .unwrap_or("Game.Config");
        write_csharp_files(&schema, data_format, namespace, &code_dir)?;
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
    project.validate_for_data()?;
    let output = required_data_output(
        project,
        format,
        &format!("coflow export {}", format.as_config_value()),
    )?;
    let dir = output_dir(project, output, options.out_dir);
    let schema = match compile_project_schema(project)? {
        Ok(schema) => schema,
        Err(diagnostics) => return Ok(PipelineOutcome::Diagnostics(diagnostics)),
    };
    let load_output = match load_project_excel(project, &schema) {
        Ok(output) => output,
        Err(diagnostics) => return Ok(PipelineOutcome::Diagnostics(diagnostics)),
    };
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
    project.validate_for_codegen()?;
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
    write_csharp_files(&schema, data_format, namespace, &dir)?;
    Ok(PipelineOutcome::Success(CodegenReport { target, dir }))
}
