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
use cli_output::{display_path, project_path, write_json_diagnostics, write_project_diagnostics};
use coflow::commands::{build_project, check_project, generate_project_code, CommandOutcome};
use coflow_runtime::DiagnosticSet;
use coflow_runtime::{normalize_path, path_to_slash, Project};
use coflow_runtime::{ProjectRuntime, SchemaTextOverride};
use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;

mod cli;
mod cli_output;
mod diagnostics;
mod format_command;
mod schema_commands;
mod self_update_command;
mod skill_commands;
mod write_file;

use diagnostics::cli_error;

use cli::{
    BuildArgs, CftArgs, CftCheckArgs, CftCommand, Cli, CodegenArgs, Command, FormatArgs,
    InitArgs, LspArgs, ProjectCheckArgs, SchemaArgs, SchemaCommand, SelfUpdateArgs, SkillArgs,
    SkillCommand, SkillScopeArgs,
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
        Command::Format(args) => format_project(&args),
        Command::Cft(command) => run_cft(&command),
        Command::Lsp(args) => run_lsp(&args),
        Command::Check(args) => project_check(&args),
        Command::Build(args) => project_build(&args),
        Command::Codegen(args) => generate_code(&args),
        Command::Schema(command) => run_schema(&command),
        Command::Skill(command) => run_skill(&command),
        Command::SelfUpdate(args) => run_self_update(&args),
    }
}

fn format_project(args: &FormatArgs) -> Result<bool, DiagnosticSet> {
    format_command::run(args.config_or_dir.as_deref(), args.check)
}

fn run_self_update(args: &SelfUpdateArgs) -> Result<bool, DiagnosticSet> {
    self_update_command::run(args)
}

fn run_skill(command: &SkillArgs) -> Result<bool, DiagnosticSet> {
    match &command.command {
        SkillCommand::Install(args) => write_skill_report(
            if args.global {
                skill_commands::install_global()?
            } else {
                skill_commands::install_project(args.config_or_dir.as_deref())?
            },
            args,
        ),
        SkillCommand::Uninstall(args) => write_skill_report(
            if args.global {
                skill_commands::uninstall_global()?
            } else {
                skill_commands::uninstall_project(args.config_or_dir.as_deref())?
            },
            args,
        ),
        SkillCommand::Status(args) => write_skill_report(
            if args.global {
                skill_commands::status_global()?
            } else {
                skill_commands::status_project(args.config_or_dir.as_deref())?
            },
            args,
        ),
    }
}

fn write_skill_report(
    report: skill_commands::SkillReport,
    args: &SkillScopeArgs,
) -> Result<bool, DiagnosticSet> {
    if args.json {
        let output = serde_json::to_string_pretty(&report)
            .map_err(|error| output_error(format!("failed to serialize skill report: {error}")))?;
        println!("{output}");
    } else {
        println!(
            "{} bundled skills ({}, version {})",
            report.operation, report.scope, report.bundle_version
        );
        for target in report.targets {
            let state = if target.installed {
                "installed"
            } else {
                "not installed"
            };
            println!(
                "  {} [{}] ({})",
                target.path.display(),
                target.agents.join(", "),
                state
            );
        }
    }
    Ok(true)
}

fn run_cft(command: &CftArgs) -> Result<bool, DiagnosticSet> {
    match &command.command {
        CftCommand::Check(args) => cft_check(args),
    }
}

fn run_schema(command: &SchemaArgs) -> Result<bool, DiagnosticSet> {
    match &command.command {
        SchemaCommand::Inspect(args) => schema_commands::inspect(
            args.config_or_dir.as_deref(),
            args.type_filter.as_deref(),
            args.include_derived,
            !args.json,
        ),
        SchemaCommand::Files(args) => {
            schema_commands::files(args.config_or_dir.as_deref(), !args.json)
        }
        SchemaCommand::WriteFile(args) => schema_commands::write_file(
            args.config_or_dir.as_deref(),
            &schema_commands::SchemaWriteFileOptions {
                file: args.file.clone(),
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
                output: if args.json {
                    schema_commands::SchemaWriteOutput::Json
                } else {
                    schema_commands::SchemaWriteOutput::Human
                },
            },
        ),
    }
}

fn init_project(args: InitArgs) -> Result<bool, DiagnosticSet> {
    let dir = args.dir.unwrap_or_else(|| PathBuf::from("."));
    let outcome = coflow_runtime::init_project(&dir)?;
    println!("created {}", outcome.config_path.display());
    Ok(true)
}

fn cft_check(args: &CftCheckArgs) -> Result<bool, DiagnosticSet> {
    let project = Project::open_schema_only(args.config_or_dir.as_deref())?;
    let project_diagnostics = project.schema_diagnostic_set();
    if !project_diagnostics.is_empty() {
        write_project_diagnostics(project_diagnostics, args.json, project.root_dir())
            .map_err(output_error)?;
        return Ok(false);
    }
    let overrides = if let Some(path) = args.stdin_path.as_deref() {
        let mut source = String::new();
        std::io::stdin()
            .read_to_string(&mut source)
            .map_err(|err| cli_error("CLI-STDIN", format!("failed to read stdin: {err}")))?;
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else {
            project.root_dir().join(path)
        };
        vec![SchemaTextOverride {
            requested_module: Some(path_to_slash(path)),
            normalized_path: normalize_path(&absolute),
            source,
        }]
    } else {
        Vec::new()
    };
    let mut runtime = ProjectRuntime::new(project.clone());
    let refresh = runtime.refresh_with_overrides(&overrides);
    let diagnostics = if let Some(attempt) = runtime.latest_attempt() {
        attempt.diagnostics().clone().into_set()
    } else {
        refresh?;
        DiagnosticSet::empty()
    };
    let success = diagnostics.is_empty();
    if success && !args.json {
        println!(
            "CFT check passed: {}",
            project_path(&project, project.config_path())
        );
    } else {
        write_project_diagnostics(diagnostics, args.json, project.root_dir())
            .map_err(output_error)?;
    }
    Ok(success)
}

fn run_lsp(args: &LspArgs) -> Result<bool, DiagnosticSet> {
    let project = Project::open_schema_only(args.config_or_dir.as_deref())?;
    coflow::lsp::run(project).map_err(|message| cli_error("LSP-RUNTIME", message))
}

fn project_check(args: &ProjectCheckArgs) -> Result<bool, DiagnosticSet> {
    let project = Project::open_schema_only(args.config_or_dir.as_deref())?;
    let root_dir = project.root_dir().to_path_buf();
    let config_path = project.config_path().to_path_buf();
    match check_project(&project)? {
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
    let project = Project::open_schema_only(args.config_or_dir.as_deref())?;
    let root_dir = project.root_dir().to_path_buf();
    let config_path = project.config_path().to_path_buf();
    match build_project(&project)? {
        CommandOutcome::Success(report) => {
            for target in report.targets {
                println!(
                    "{} code generated to {}",
                    target.code.display_name,
                    display_path(&target.code.dir.display().to_string(), Some(&root_dir))
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

fn generate_code(args: &CodegenArgs) -> Result<bool, DiagnosticSet> {
    let project = Project::open_schema_only(args.config_or_dir.as_deref())?;
    let root_dir = project.root_dir().to_path_buf();
    match generate_project_code(&project)? {
        CommandOutcome::Success(report) => {
            for target in report.targets {
                println!(
                    "{} code generated to {}",
                    target.display_name,
                    display_path(&target.dir.display().to_string(), Some(&root_dir))
                );
            }
            Ok(true)
        }
        CommandOutcome::Diagnostics(diagnostics) => {
            write_project_diagnostics(diagnostics, false, &root_dir).map_err(output_error)?;
            Ok(false)
        }
    }
}

fn output_error(message: String) -> DiagnosticSet {
    cli_error("CLI-OUTPUT", message)
}
