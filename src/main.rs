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

use clap::{Args, Parser, Subcommand};
use coflow_cft::CftDiagnostic;
use coflow_pipeline::{
    build_project, check_project, export_project_data, generate_project_code, BuildOptions,
    CodegenOptions, CodegenTarget, DataFormat, ExportOptions, PipelineOutcome,
};
use coflow_project::{compile_schema_project, dedupe_cft_diagnostics, DiagnosticJson, Project};
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(message) => {
            let _ = writeln!(io::stderr().lock(), "{message}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<bool, String> {
    match Cli::parse().command {
        Command::Init(args) => init_project(args),
        Command::Cft(command) => run_cft(&command),
        Command::Check(args) => project_check(&args),
        Command::Build(args) => project_build(&args),
        Command::Export(command) => run_export(&command),
        Command::Codegen(command) => run_codegen(&command),
    }
}

fn run_cft(command: &CftArgs) -> Result<bool, String> {
    match &command.command {
        CftCommand::Check(args) => cft_check(args),
        CftCommand::Lsp(args) => cft_lsp(args),
    }
}

fn run_export(command: &ExportArgs) -> Result<bool, String> {
    match &command.command {
        ExportCommand::Json(args) => export_json(args),
        ExportCommand::Messagepack(args) => export_messagepack(args),
    }
}

fn run_codegen(command: &CodegenArgs) -> Result<bool, String> {
    match &command.command {
        CodegenCommand::Csharp(args) => codegen_csharp(args),
    }
}

#[derive(Debug, Parser)]
#[command(name = "coflow")]
#[command(about = "Project-level tools for Coflow schemas and data.")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Create a minimal Coflow project.
    Init(InitArgs),
    /// CFT schema tools.
    Cft(CftArgs),
    /// Run the full project validation pipeline.
    Check(ProjectCheckArgs),
    /// Run validation, data export, and configured code generation.
    Build(BuildArgs),
    /// Export project data.
    Export(ExportArgs),
    /// Generate runtime code.
    Codegen(CodegenArgs),
}

#[derive(Debug, Args)]
struct InitArgs {
    #[arg(value_name = "DIR")]
    dir: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct CftArgs {
    #[command(subcommand)]
    command: CftCommand,
}

#[derive(Debug, Subcommand)]
enum CftCommand {
    /// Compile all CFT schema files from coflow.yaml.
    Check(CftCheckArgs),
    /// Start the CFT language server.
    Lsp(CftLspArgs),
}

#[derive(Debug, Args)]
struct CftCheckArgs {
    #[arg(value_name = "CONFIG_OR_DIR")]
    config_or_dir: Option<PathBuf>,
    /// Emit machine-readable diagnostics JSON.
    #[arg(long)]
    json: bool,
    /// Treat stdin as this schema file's source.
    #[arg(long = "stdin-path", value_name = "PATH")]
    stdin_path: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct CftLspArgs {
    #[arg(value_name = "CONFIG_OR_DIR")]
    config_or_dir: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct ProjectCheckArgs {
    #[arg(value_name = "CONFIG_OR_DIR")]
    config_or_dir: Option<PathBuf>,
    /// Emit machine-readable diagnostics JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct BuildArgs {
    #[arg(value_name = "CONFIG_OR_DIR")]
    config_or_dir: Option<PathBuf>,
    /// Override outputs.data.dir for this invocation.
    #[arg(long = "data-out", value_name = "DIR")]
    data_out_dir: Option<PathBuf>,
    /// Override outputs.code.dir for this invocation.
    #[arg(long = "code-out", value_name = "DIR")]
    code_out_dir: Option<PathBuf>,
    /// Override outputs.code.namespace for this invocation.
    #[arg(long, value_name = "NAME")]
    namespace: Option<String>,
}

#[derive(Debug, Args)]
struct ExportArgs {
    #[command(subcommand)]
    command: ExportCommand,
}

#[derive(Debug, Subcommand)]
enum ExportCommand {
    /// Export data as JSON. The project config must declare outputs.data.type: json.
    Json(ExportJsonArgs),
    /// Export data as `MessagePack`. The project config must declare outputs.data.type: messagepack.
    Messagepack(ExportMessagePackArgs),
}

#[derive(Debug, Args)]
struct ExportJsonArgs {
    #[arg(value_name = "CONFIG_OR_DIR")]
    config_or_dir: Option<PathBuf>,
    /// Override outputs.data.dir for this invocation.
    #[arg(long = "out", value_name = "DIR")]
    out_dir: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct ExportMessagePackArgs {
    #[arg(value_name = "CONFIG_OR_DIR")]
    config_or_dir: Option<PathBuf>,
    /// Override outputs.data.dir for this invocation.
    #[arg(long = "out", value_name = "DIR")]
    out_dir: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct CodegenArgs {
    #[command(subcommand)]
    command: CodegenCommand,
}

#[derive(Debug, Subcommand)]
enum CodegenCommand {
    /// Generate C# runtime code. The project config must declare outputs.code.type: csharp.
    Csharp(CodegenCsharpArgs),
}

#[derive(Debug, Args)]
struct CodegenCsharpArgs {
    #[arg(value_name = "CONFIG_OR_DIR")]
    config_or_dir: Option<PathBuf>,
    /// Override outputs.code.dir for this invocation.
    #[arg(long = "out", value_name = "DIR")]
    out_dir: Option<PathBuf>,
    /// Override outputs.code.namespace for this invocation.
    #[arg(long, value_name = "NAME")]
    namespace: Option<String>,
}

fn init_project(args: InitArgs) -> Result<bool, String> {
    let dir = args.dir.unwrap_or_else(|| PathBuf::from("."));
    let config_path = dir.join("coflow.yaml");
    if config_path.exists() {
        return Err(format!("`{}` already exists", config_path.display()));
    }
    fs::create_dir_all(dir.join("schema"))
        .map_err(|err| format!("failed to create `{}`: {err}", dir.join("schema").display()))?;
    fs::create_dir_all(dir.join("data"))
        .map_err(|err| format!("failed to create `{}`: {err}", dir.join("data").display()))?;
    fs::create_dir_all(dir.join("generated").join("data")).map_err(|err| {
        format!(
            "failed to create `{}`: {err}",
            dir.join("generated").join("data").display()
        )
    })?;
    fs::create_dir_all(dir.join("generated").join("csharp")).map_err(|err| {
        format!(
            "failed to create `{}`: {err}",
            dir.join("generated").join("csharp").display()
        )
    })?;
    let config = r"schema: schema/

sources: []

outputs:
  data:
    type: json
    dir: generated/data
  code:
    type: csharp
    dir: generated/csharp
    namespace: Game.Config
";
    fs::write(&config_path, config)
        .map_err(|err| format!("failed to write `{}`: {err}", config_path.display()))?;
    println!("created {}", config_path.display());
    Ok(true)
}

fn cft_check(args: &CftCheckArgs) -> Result<bool, String> {
    let project = Project::open_schema_only(args.config_or_dir.as_deref())?;
    let build = compile_schema_project(&project, args.stdin_path.as_deref())?;
    let diagnostics = dedupe_cft_diagnostics(build.diagnostics);
    if args.json {
        write_json_diagnostics(
            diagnostics
                .iter()
                .map(|diagnostic| {
                    DiagnosticJson::from_cft(diagnostic, &build.sources, &build.paths)
                })
                .collect(),
        )?;
    } else if diagnostics.is_empty() {
        println!(
            "CFT check passed: {}",
            project
                .config_path
                .strip_prefix(&project.root_dir)
                .map_or_else(
                    |_| project.config_path.display().to_string(),
                    |path| path.display().to_string()
                )
        );
    } else {
        write_human_cft_diagnostics(&diagnostics, &build.sources, &build.paths)?;
    }
    Ok(diagnostics.is_empty())
}

fn cft_lsp(args: &CftLspArgs) -> Result<bool, String> {
    let project = Project::open_schema_only(args.config_or_dir.as_deref())?;
    coflow_cft_lsp::run(project)
}

fn project_check(args: &ProjectCheckArgs) -> Result<bool, String> {
    let project = Project::open_schema_only(args.config_or_dir.as_deref())?;
    match check_project(&project)? {
        PipelineOutcome::Success(_) => {
            if args.json {
                write_json_diagnostics(Vec::new())?;
            } else {
                println!("Project check passed: {}", project.config_path.display());
            }
            Ok(true)
        }
        PipelineOutcome::Diagnostics(diagnostics) => {
            write_project_diagnostics(diagnostics, args.json)?;
            Ok(false)
        }
    }
}

fn project_build(args: &BuildArgs) -> Result<bool, String> {
    let project = Project::open_schema_only(args.config_or_dir.as_deref())?;
    match build_project(
        &project,
        BuildOptions {
            data_out_dir: args.data_out_dir.as_deref(),
            code_out_dir: args.code_out_dir.as_deref(),
            namespace: args.namespace.as_deref(),
        },
    )? {
        PipelineOutcome::Success(report) => {
            println!(
                "{} data exported to {}",
                report.data.format.display_name(),
                report.data.dir.display()
            );
            if let Some(code) = report.code {
                println!(
                    "{} code generated to {}",
                    code.target.display_name(),
                    code.dir.display()
                );
            }
            println!("Build completed: {}", project.config_path.display());
            Ok(true)
        }
        PipelineOutcome::Diagnostics(diagnostics) => {
            write_project_diagnostics(diagnostics, false)?;
            Ok(false)
        }
    }
}

fn export_json(args: &ExportJsonArgs) -> Result<bool, String> {
    let project = Project::open_schema_only(args.config_or_dir.as_deref())?;
    match export_project_data(
        &project,
        DataFormat::Json,
        ExportOptions {
            out_dir: args.out_dir.as_deref(),
        },
    )? {
        PipelineOutcome::Success(report) => {
            println!("JSON data exported to {}", report.dir.display());
            Ok(true)
        }
        PipelineOutcome::Diagnostics(diagnostics) => {
            write_project_diagnostics(diagnostics, false)?;
            Ok(false)
        }
    }
}

fn export_messagepack(args: &ExportMessagePackArgs) -> Result<bool, String> {
    let project = Project::open_schema_only(args.config_or_dir.as_deref())?;
    match export_project_data(
        &project,
        DataFormat::Messagepack,
        ExportOptions {
            out_dir: args.out_dir.as_deref(),
        },
    )? {
        PipelineOutcome::Success(report) => {
            println!("MessagePack data exported to {}", report.dir.display());
            Ok(true)
        }
        PipelineOutcome::Diagnostics(diagnostics) => {
            write_project_diagnostics(diagnostics, false)?;
            Ok(false)
        }
    }
}

fn codegen_csharp(args: &CodegenCsharpArgs) -> Result<bool, String> {
    let project = Project::open_schema_only(args.config_or_dir.as_deref())?;
    match generate_project_code(
        &project,
        CodegenTarget::Csharp,
        CodegenOptions {
            out_dir: args.out_dir.as_deref(),
            namespace: args.namespace.as_deref(),
        },
    )? {
        PipelineOutcome::Success(report) => {
            println!("C# code generated to {}", report.dir.display());
            Ok(true)
        }
        PipelineOutcome::Diagnostics(diagnostics) => {
            write_project_diagnostics(diagnostics, false)?;
            Ok(false)
        }
    }
}

#[derive(Debug, Default, Serialize)]
struct DiagnosticsOutput {
    diagnostics: Vec<DiagnosticJson>,
}

fn write_json_diagnostics(diagnostics: Vec<DiagnosticJson>) -> Result<(), String> {
    serde_json::to_writer(io::stdout().lock(), &DiagnosticsOutput { diagnostics })
        .map_err(|err| format!("failed to write diagnostics JSON: {err}"))?;
    println!();
    Ok(())
}

fn write_project_diagnostics(diagnostics: Vec<DiagnosticJson>, json: bool) -> Result<(), String> {
    if json {
        write_json_diagnostics(diagnostics)
    } else {
        write_human_project_diagnostics(&diagnostics)
    }
}

fn write_human_project_diagnostics(diagnostics: &[DiagnosticJson]) -> Result<(), String> {
    let mut stderr = io::stderr().lock();
    for diagnostic in diagnostics {
        writeln!(
            stderr,
            "{} [{}] {}:{}:{} {}",
            diagnostic.code,
            diagnostic.stage,
            diagnostic.path,
            diagnostic.start_line + 1,
            diagnostic.start_character + 1,
            diagnostic.message
        )
        .map_err(|err| format!("failed to write diagnostics: {err}"))?;
    }
    Ok(())
}

fn write_human_cft_diagnostics(
    diagnostics: &[CftDiagnostic],
    sources: &BTreeMap<String, String>,
    paths: &BTreeMap<String, String>,
) -> Result<(), String> {
    let mut stderr = io::stderr().lock();
    for diagnostic in diagnostics {
        let json = DiagnosticJson::from_cft(diagnostic, sources, paths);
        writeln!(
            stderr,
            "{} [{}] {}:{}:{} {}",
            diagnostic.code.as_str(),
            diagnostic.stage,
            json.path,
            json.start_line + 1,
            json.start_character + 1,
            diagnostic.message
        )
        .map_err(|err| format!("failed to write diagnostics: {err}"))?;
    }
    Ok(())
}
