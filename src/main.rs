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

use clap::Parser;
use cli_output::{
    display_path, project_path, write_human_cft_diagnostics, write_json_diagnostics,
    write_project_diagnostics,
};
use coflow::diagnostics::cli_error;
use coflow::commands::{
    build_project, check_project, export_project_data, generate_project_code, BuildOptions,
    CodegenOptions, CommandOutcome, ExportOptions, CSHARP_CODEGEN_ID, JSON_EXPORTER_ID,
    MESSAGEPACK_EXPORTER_ID,
};
use coflow::diagnostics::DiagnosticJson;
use coflow::{data_commands, schema_commands};
use coflow_api::DiagnosticSet;
use coflow_project::{compile_schema_project, dedupe_cft_diagnostics, Project};
use data_get_target::parse_data_get_target;
use serde_json::Value;
use std::path::PathBuf;
use std::process::ExitCode;

mod cli;
mod cli_output;
mod data_get_target;

use cli::{
    BuildArgs, CftArgs, CftCheckArgs, CftCommand, Cli, CodegenArgs, CodegenCommand,
    CodegenCsharpArgs, Command, DataArgs, DataCommand, ExportArgs, ExportCommand, ExportJsonArgs,
    ExportMessagePackArgs, InitArgs, LspArgs, ProjectCheckArgs, SchemaArgs, SchemaCommand,
};

fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(diagnostics) => {
            let _ = write_project_diagnostics(diagnostics, false, PathBuf::from(".").as_path());
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<bool, DiagnosticSet> {
    match Cli::parse().command {
        Command::Init(args) => init_project(args),
        Command::Cft(command) => run_cft(&command),
        Command::Lsp(args) => run_lsp(&args),
        Command::Check(args) => project_check(&args),
        Command::Build(args) => project_build(&args),
        Command::Export(command) => run_export(&command),
        Command::Codegen(command) => run_codegen(&command),
        Command::Schema(command) => run_schema(&command),
        Command::Data(command) => run_data(&command),
    }
}

fn run_cft(command: &CftArgs) -> Result<bool, DiagnosticSet> {
    match &command.command {
        CftCommand::Check(args) => cft_check(args),
    }
}

fn run_export(command: &ExportArgs) -> Result<bool, DiagnosticSet> {
    match &command.command {
        ExportCommand::Json(args) => export_json(args),
        ExportCommand::Messagepack(args) => export_messagepack(args),
    }
}

fn run_codegen(command: &CodegenArgs) -> Result<bool, DiagnosticSet> {
    match &command.command {
        CodegenCommand::Csharp(args) => codegen_csharp(args),
    }
}

fn run_schema(command: &SchemaArgs) -> Result<bool, DiagnosticSet> {
    match &command.command {
        SchemaCommand::Inspect(args) => schema_commands::inspect(
            args.config_or_dir.as_deref(),
            args.type_filter.as_deref(),
            args.include_derived,
            args.human,
        ),
        SchemaCommand::Files(args) => {
            schema_commands::files(args.config_or_dir.as_deref(), args.human)
        }
        SchemaCommand::WriteFile(args) => schema_commands::write_file(
            args.config_or_dir.as_deref(),
            &schema_commands::SchemaWriteFileOptions {
                file: args.file.clone(),
                input: if args.stdin {
                    schema_commands::SchemaWriteInput::Stdin
                } else {
                    schema_commands::SchemaWriteInput::Missing
                },
                mode: if args.dry_run {
                    schema_commands::SchemaWriteMode::DryRun
                } else {
                    schema_commands::SchemaWriteMode::Write
                },
                check: if args.check {
                    schema_commands::SchemaWriteCheck::Run
                } else {
                    schema_commands::SchemaWriteCheck::Skip
                },
                output: if args.human {
                    schema_commands::SchemaWriteOutput::Human
                } else {
                    schema_commands::SchemaWriteOutput::Json
                },
            },
        ),
    }
}

fn run_data(command: &DataArgs) -> Result<bool, DiagnosticSet> {
    match &command.command {
        DataCommand::Sources(args) => {
            data_commands::sources(args.config_or_dir.as_deref(), args.human)
        }
        DataCommand::List(args) => data_commands::list(
            args.config_or_dir.as_deref(),
            args.actual_type.clone(),
            args.file.clone(),
            args.limit,
            args.offset,
            args.human,
        ),
        DataCommand::Get(args) => {
            let target = parse_data_get_target(&args.target).map_err(cli_arg_error)?;
            data_commands::get(data_commands::DataGetOptions {
                config_or_dir: target.config_or_dir,
                selector: target.selector,
                actual_type: args.actual_type.clone(),
                file: args.file.clone(),
                keys: split_keys(&args.keys),
                limit: args.limit,
                offset: args.offset,
                all: args.all,
                human: args.human,
            })
        }
        DataCommand::Patch(args) => {
            data_commands::patch(args.config_or_dir.as_deref(), &args.patch, args.human)
        }
        DataCommand::CreateFile(args) => data_commands::create_file(
            args.config_or_dir.as_deref(),
            args.file.clone(),
            args.actual_type.clone(),
            args.provider.clone(),
            args.sheet.clone(),
            args.human,
        ),
        DataCommand::CreateTable(args) => data_commands::create_table(
            args.config_or_dir.as_deref(),
            args.source.clone(),
            args.actual_type.clone(),
            args.provider.as_deref(),
            args.sheet.clone(),
            args.human,
        ),
        DataCommand::SyncHeader(args) => data_commands::sync_header(
            args.config_or_dir.as_deref(),
            args.file.clone(),
            args.actual_type.clone(),
            args.provider.clone(),
            args.sheet.clone(),
            args.human,
        ),
        DataCommand::WriteFile(args) => data_commands::write_file(
            args.config_or_dir.as_deref(),
            &data_commands::DataWriteFileOptions {
                file: args.file.clone(),
                input: if args.stdin {
                    data_commands::DataWriteInput::Stdin
                } else {
                    data_commands::DataWriteInput::Missing
                },
                mode: if args.dry_run {
                    data_commands::DataWriteMode::DryRun
                } else {
                    data_commands::DataWriteMode::Write
                },
                check: if args.check {
                    data_commands::DataWriteCheck::Run
                } else {
                    data_commands::DataWriteCheck::Skip
                },
                output: if args.human {
                    data_commands::DataWriteOutput::Human
                } else {
                    data_commands::DataWriteOutput::Json
                },
            },
        ),
    }
}

fn init_project(args: InitArgs) -> Result<bool, DiagnosticSet> {
    let dir = args.dir.unwrap_or_else(|| PathBuf::from("."));
    let outcome = coflow_project::init_project(&dir)?;
    println!("created {}", outcome.config_path.display());
    Ok(true)
}

fn split_keys(keys: &[String]) -> Vec<String> {
    keys.iter()
        .flat_map(|key| key.split(','))
        .filter(|key| !key.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn cft_check(args: &CftCheckArgs) -> Result<bool, DiagnosticSet> {
    let project = Project::open_schema_only(args.config_or_dir.as_deref())?;
    let project_diagnostics = project.schema_diagnostic_set();
    if !project_diagnostics.is_empty() {
        write_project_diagnostics(project_diagnostics, args.json, &project.root_dir)
            .map_err(output_error)?;
        return Ok(false);
    }
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
        )
        .map_err(output_error)?;
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
        )
        .map_err(output_error)?;
    }
    Ok(diagnostics.is_empty())
}

fn run_lsp(args: &LspArgs) -> Result<bool, DiagnosticSet> {
    let project = Project::open_schema_only(args.config_or_dir.as_deref())?;
    coflow_lsp::run(project).map_err(|message| cli_error("LSP-RUNTIME", message))
}

fn project_check(args: &ProjectCheckArgs) -> Result<bool, DiagnosticSet> {
    let project = Project::open_schema_only(args.config_or_dir.as_deref())?;
    let root_dir = project.root_dir.clone();
    let config_path = project.config_path.clone();
    let registry = default_provider_registry()?;
    match check_project(project, &registry)? {
        CommandOutcome::Success(_) => {
            if args.json {
                write_json_diagnostics(Vec::new()).map_err(output_error)?;
            } else {
                println!(
                    "Project check passed: {}",
                    display_path(&config_path.display().to_string(), Some(&root_dir))
                );
            }
            Ok(true)
        }
        CommandOutcome::Diagnostics(diagnostics) => {
            write_project_diagnostics(diagnostics, args.json, &root_dir).map_err(output_error)?;
            Ok(false)
        }
    }
}

fn project_build(args: &BuildArgs) -> Result<bool, DiagnosticSet> {
    let mut project = Project::open_schema_only(args.config_or_dir.as_deref())?;
    override_code_namespace(&mut project, args.namespace.as_deref());
    let root_dir = project.root_dir.clone();
    let config_path = project.config_path.clone();
    let registry = default_provider_registry()?;
    match build_project(
        project,
        &registry,
        BuildOptions {
            data_out_dir: args.data_out_dir.as_deref(),
            code_out_dir: args.code_out_dir.as_deref(),
        },
    )? {
        CommandOutcome::Success(report) => {
            println!(
                "{} data exported to {}",
                report.data.display_name,
                display_path(&report.data.dir.display().to_string(), Some(&root_dir))
            );
            if let Some(code) = report.code {
                println!(
                    "{} code generated to {}",
                    code.display_name,
                    display_path(&code.dir.display().to_string(), Some(&root_dir))
                );
            }
            println!(
                "Build completed: {}",
                display_path(&config_path.display().to_string(), Some(&root_dir))
            );
            Ok(true)
        }
        CommandOutcome::Diagnostics(diagnostics) => {
            write_project_diagnostics(diagnostics, false, &root_dir).map_err(output_error)?;
            Ok(false)
        }
    }
}

fn export_json(args: &ExportJsonArgs) -> Result<bool, DiagnosticSet> {
    let project = Project::open_schema_only(args.config_or_dir.as_deref())?;
    let root_dir = project.root_dir.clone();
    let registry = default_provider_registry()?;
    match export_project_data(
        project,
        &registry,
        JSON_EXPORTER_ID,
        ExportOptions {
            out_dir: args.out_dir.as_deref(),
        },
    )? {
        CommandOutcome::Success(report) => {
            println!(
                "JSON data exported to {}",
                display_path(&report.dir.display().to_string(), Some(&root_dir))
            );
            Ok(true)
        }
        CommandOutcome::Diagnostics(diagnostics) => {
            write_project_diagnostics(diagnostics, false, &root_dir).map_err(output_error)?;
            Ok(false)
        }
    }
}

fn export_messagepack(args: &ExportMessagePackArgs) -> Result<bool, DiagnosticSet> {
    let project = Project::open_schema_only(args.config_or_dir.as_deref())?;
    let root_dir = project.root_dir.clone();
    let registry = default_provider_registry()?;
    match export_project_data(
        project,
        &registry,
        MESSAGEPACK_EXPORTER_ID,
        ExportOptions {
            out_dir: args.out_dir.as_deref(),
        },
    )? {
        CommandOutcome::Success(report) => {
            println!(
                "MessagePack data exported to {}",
                display_path(&report.dir.display().to_string(), Some(&root_dir))
            );
            Ok(true)
        }
        CommandOutcome::Diagnostics(diagnostics) => {
            write_project_diagnostics(diagnostics, false, &root_dir).map_err(output_error)?;
            Ok(false)
        }
    }
}

fn codegen_csharp(args: &CodegenCsharpArgs) -> Result<bool, DiagnosticSet> {
    let mut project = Project::open_schema_only(args.config_or_dir.as_deref())?;
    override_code_namespace(&mut project, args.namespace.as_deref());
    let root_dir = project.root_dir.clone();
    let registry = default_provider_registry()?;
    match generate_project_code(
        project,
        &registry,
        CSHARP_CODEGEN_ID,
        CodegenOptions {
            out_dir: args.out_dir.as_deref(),
        },
    )? {
        CommandOutcome::Success(report) => {
            println!(
                "C# code generated to {}",
                display_path(&report.dir.display().to_string(), Some(&root_dir))
            );
            Ok(true)
        }
        CommandOutcome::Diagnostics(diagnostics) => {
            write_project_diagnostics(diagnostics, false, &root_dir).map_err(output_error)?;
            Ok(false)
        }
    }
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

fn cli_arg_error(message: String) -> DiagnosticSet {
    cli_error("CLI-ARG", message)
}

fn output_error(message: String) -> DiagnosticSet {
    cli_error("CLI-OUTPUT", message)
}

fn default_provider_registry() -> Result<coflow_api::ProviderRegistry, DiagnosticSet> {
    coflow_builtins::default_provider_registry()
        .map_err(|err| cli_error("PROVIDER-REGISTRY", err.to_string()))
}
