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

use clap::{Args, Parser, Subcommand};
use coflow_cft::CftDiagnostic;
use coflow_pipeline::{
    build_project, check_project, export_project_data, generate_project_code, BuildOptions,
    CodegenOptions, ExportOptions, PipelineOutcome, CSHARP_CODEGEN_ID, JSON_EXPORTER_ID,
    MESSAGEPACK_EXPORTER_ID,
};
use coflow_project::{
    compile_schema_project, dedupe_cft_diagnostics, diagnostic_json_from_set, DiagnosticJson,
    Project,
};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const DIAGNOSTIC_SEPARATOR: &str = "----------------------------------------";

fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(message) => {
            let _ = write_cli_error(&message);
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<bool, String> {
    match Cli::parse().command {
        Command::Init(args) => init_project(args),
        Command::Cft(command) => run_cft(&command),
        Command::Lsp(args) => run_lsp(&args),
        Command::Check(args) => project_check(&args),
        Command::Build(args) => project_build(&args),
        Command::Export(command) => run_export(&command),
        Command::Codegen(command) => run_codegen(&command),
    }
}

fn run_cft(command: &CftArgs) -> Result<bool, String> {
    match &command.command {
        CftCommand::Check(args) => cft_check(args),
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
    /// Start the Coflow language server (CFT + CFD).
    Lsp(LspArgs),
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
struct LspArgs {
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
    let outcome = coflow_project::init_project(&dir)?;
    println!("created {}", outcome.config_path.display());
    Ok(true)
}

fn cft_check(args: &CftCheckArgs) -> Result<bool, String> {
    let project = Project::open_schema_only(args.config_or_dir.as_deref())?;
    let project_diagnostics = project.schema_diagnostic_set();
    if !project_diagnostics.is_empty() {
        write_project_diagnostics(project_diagnostics, args.json, &project.root_dir)?;
        return Ok(false);
    }
    let build = compile_schema_project(&project, args.stdin_path.as_deref())
        .map_err(|message| relativize_message_paths(&message, &project.root_dir))?;
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
            project_path(&project, &project.config_path)
        );
    } else {
        write_human_cft_diagnostics(
            &diagnostics,
            &build.sources,
            &build.paths,
            &project.root_dir,
        )?;
    }
    Ok(diagnostics.is_empty())
}

fn run_lsp(args: &LspArgs) -> Result<bool, String> {
    let project = Project::open_schema_only(args.config_or_dir.as_deref())?;
    let root_dir = project.root_dir.clone();
    coflow_lsp::run(project).map_err(|message| relativize_message_paths(&message, &root_dir))
}

fn project_check(args: &ProjectCheckArgs) -> Result<bool, String> {
    let project = Project::open_schema_only(args.config_or_dir.as_deref())?;
    let registry = coflow::builtin_registry().map_err(|err| err.to_string())?;
    match check_project(&project, &registry)
        .map_err(|message| relativize_message_paths(&message, &project.root_dir))?
    {
        PipelineOutcome::Success(_) => {
            if args.json {
                write_json_diagnostics(Vec::new())?;
            } else {
                println!(
                    "Project check passed: {}",
                    project_path(&project, &project.config_path)
                );
            }
            Ok(true)
        }
        PipelineOutcome::Diagnostics(diagnostics) => {
            write_project_diagnostics(diagnostics, args.json, &project.root_dir)?;
            Ok(false)
        }
    }
}

fn project_build(args: &BuildArgs) -> Result<bool, String> {
    let mut project = Project::open_schema_only(args.config_or_dir.as_deref())?;
    override_code_namespace(&mut project, args.namespace.as_deref());
    let registry = coflow::builtin_registry().map_err(|err| err.to_string())?;
    match build_project(
        &project,
        &registry,
        BuildOptions {
            data_out_dir: args.data_out_dir.as_deref(),
            code_out_dir: args.code_out_dir.as_deref(),
        },
    )
    .map_err(|message| relativize_message_paths(&message, &project.root_dir))?
    {
        PipelineOutcome::Success(report) => {
            println!(
                "{} data exported to {}",
                report.data.display_name,
                project_path(&project, &report.data.dir)
            );
            if let Some(code) = report.code {
                println!(
                    "{} code generated to {}",
                    code.display_name,
                    project_path(&project, &code.dir)
                );
            }
            println!(
                "Build completed: {}",
                project_path(&project, &project.config_path)
            );
            Ok(true)
        }
        PipelineOutcome::Diagnostics(diagnostics) => {
            write_project_diagnostics(diagnostics, false, &project.root_dir)?;
            Ok(false)
        }
    }
}

fn export_json(args: &ExportJsonArgs) -> Result<bool, String> {
    let project = Project::open_schema_only(args.config_or_dir.as_deref())?;
    let registry = coflow::builtin_registry().map_err(|err| err.to_string())?;
    match export_project_data(
        &project,
        &registry,
        JSON_EXPORTER_ID,
        ExportOptions {
            out_dir: args.out_dir.as_deref(),
        },
    )
    .map_err(|message| relativize_message_paths(&message, &project.root_dir))?
    {
        PipelineOutcome::Success(report) => {
            println!(
                "JSON data exported to {}",
                project_path(&project, &report.dir)
            );
            Ok(true)
        }
        PipelineOutcome::Diagnostics(diagnostics) => {
            write_project_diagnostics(diagnostics, false, &project.root_dir)?;
            Ok(false)
        }
    }
}

fn export_messagepack(args: &ExportMessagePackArgs) -> Result<bool, String> {
    let project = Project::open_schema_only(args.config_or_dir.as_deref())?;
    let registry = coflow::builtin_registry().map_err(|err| err.to_string())?;
    match export_project_data(
        &project,
        &registry,
        MESSAGEPACK_EXPORTER_ID,
        ExportOptions {
            out_dir: args.out_dir.as_deref(),
        },
    )
    .map_err(|message| relativize_message_paths(&message, &project.root_dir))?
    {
        PipelineOutcome::Success(report) => {
            println!(
                "MessagePack data exported to {}",
                project_path(&project, &report.dir)
            );
            Ok(true)
        }
        PipelineOutcome::Diagnostics(diagnostics) => {
            write_project_diagnostics(diagnostics, false, &project.root_dir)?;
            Ok(false)
        }
    }
}

fn codegen_csharp(args: &CodegenCsharpArgs) -> Result<bool, String> {
    let mut project = Project::open_schema_only(args.config_or_dir.as_deref())?;
    override_code_namespace(&mut project, args.namespace.as_deref());
    let registry = coflow::builtin_registry().map_err(|err| err.to_string())?;
    match generate_project_code(
        &project,
        &registry,
        CSHARP_CODEGEN_ID,
        CodegenOptions {
            out_dir: args.out_dir.as_deref(),
        },
    )
    .map_err(|message| relativize_message_paths(&message, &project.root_dir))?
    {
        PipelineOutcome::Success(report) => {
            println!(
                "C# code generated to {}",
                project_path(&project, &report.dir)
            );
            Ok(true)
        }
        PipelineOutcome::Diagnostics(diagnostics) => {
            write_project_diagnostics(diagnostics, false, &project.root_dir)?;
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

fn override_code_namespace(project: &mut Project, namespace: Option<&str>) {
    let Some(namespace) = namespace else {
        return;
    };
    if let Some(output) = project.config.outputs.code.as_mut() {
        let mut options = output.options().as_object().cloned().unwrap_or_default();
        options.insert(
            "namespace".to_string(),
            Value::String(namespace.to_string()),
        );
        output.options = Value::Object(options);
    }
}

fn write_project_diagnostics(
    diagnostics: coflow_api::DiagnosticSet,
    json: bool,
    root_dir: &Path,
) -> Result<(), String> {
    let diagnostics = diagnostic_json_from_set(diagnostics);
    if json {
        write_json_diagnostics(diagnostics)
    } else {
        write_human_diagnostics(&diagnostics, Some(root_dir))
    }
}

fn write_human_diagnostics(
    diagnostics: &[DiagnosticJson],
    root_dir: Option<&Path>,
) -> Result<(), String> {
    let mut stderr = io::stderr().lock();
    for diagnostic in diagnostics {
        write_diagnostic_block(&mut stderr, diagnostic, root_dir)
            .map_err(|err| format!("failed to write diagnostics: {err}"))?;
    }
    Ok(())
}

fn write_human_cft_diagnostics(
    diagnostics: &[CftDiagnostic],
    sources: &BTreeMap<String, String>,
    paths: &BTreeMap<String, String>,
    root_dir: &Path,
) -> Result<(), String> {
    let diagnostics = diagnostics
        .iter()
        .map(|diagnostic| DiagnosticJson::from_cft(diagnostic, sources, paths))
        .collect::<Vec<_>>();
    write_human_diagnostics(&diagnostics, Some(root_dir))
}

fn write_cli_error(message: &str) -> Result<(), String> {
    let mut stderr = io::stderr().lock();
    writeln!(stderr, "{DIAGNOSTIC_SEPARATOR}")
        .map_err(|err| format!("failed to write diagnostics: {err}"))?;
    writeln!(stderr, "[CLI-ERROR] [CLI]")
        .map_err(|err| format!("failed to write diagnostics: {err}"))?;
    write_message_field(&mut stderr, message)
        .map_err(|err| format!("failed to write diagnostics: {err}"))
}

fn write_diagnostic_block(
    stderr: &mut impl Write,
    diagnostic: &DiagnosticJson,
    root_dir: Option<&Path>,
) -> io::Result<()> {
    writeln!(stderr, "{DIAGNOSTIC_SEPARATOR}")?;
    writeln!(stderr, "[{}] [{}]", diagnostic.code, diagnostic.stage)?;
    if !diagnostic.path.is_empty() {
        writeln!(
            stderr,
            "{:<8}{}",
            "file",
            display_path(&diagnostic.path, root_dir)
        )?;
    }
    if let Some(sheet) = &diagnostic.sheet {
        writeln!(stderr, "{:<8}{sheet}", "sheet")?;
    }
    if let Some(cell) = &diagnostic.cell {
        writeln!(stderr, "{:<8}{cell}", "cell")?;
    } else {
        writeln!(stderr, "{:<8}{}", "line", diagnostic.start_line + 1)?;
        writeln!(stderr, "{:<8}{}", "column", diagnostic.start_character + 1)?;
    }
    let message = root_dir.map_or_else(
        || diagnostic.message.clone(),
        |root_dir| relativize_message_paths(&diagnostic.message, root_dir),
    );
    write_message_field(stderr, &message)
}

fn write_message_field(stderr: &mut impl Write, message: &str) -> io::Result<()> {
    writeln!(stderr, "message")?;
    for line in message.lines() {
        writeln!(stderr, "  {line}")?;
    }
    if message.is_empty() {
        writeln!(stderr, "  ")?;
    }
    Ok(())
}

fn display_path(path: &str, root_dir: Option<&Path>) -> String {
    let cleaned_path = strip_windows_extended_prefix(path);
    let path = PathBuf::from(&cleaned_path);
    if let Some(root_dir) = root_dir {
        let cleaned_root = strip_windows_extended_prefix(&root_dir.display().to_string());
        let root = PathBuf::from(cleaned_root);
        if let Ok(relative) = path.strip_prefix(&root) {
            let value = slash_path(relative);
            return if value.is_empty() {
                ".".to_string()
            } else {
                value
            };
        }
    }
    slash_path(&path)
}

fn project_path(project: &Project, path: &Path) -> String {
    display_path(&path.display().to_string(), Some(&project.root_dir))
}

fn relativize_message_paths(message: &str, root_dir: &Path) -> String {
    let mut out = String::with_capacity(message.len());
    let mut rest = message;
    while let Some(start) = rest.find('`') {
        out.push_str(&rest[..=start]);
        let after_start = &rest[start + 1..];
        let Some(end) = after_start.find('`') else {
            out.push_str(after_start);
            return out;
        };
        out.push_str(&display_path(&after_start[..end], Some(root_dir)));
        out.push('`');
        rest = &after_start[end + 1..];
    }
    out.push_str(rest);
    out
}

fn strip_windows_extended_prefix(path: &str) -> String {
    path.strip_prefix(r"\\?\").unwrap_or(path).to_string()
}

fn slash_path(path: &Path) -> String {
    path.display().to_string().replace('\\', "/")
}
