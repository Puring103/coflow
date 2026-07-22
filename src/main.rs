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
use coflow::commands::{
    build_project, check_project, clean_project, export_project_data, generate_project_code,
    BuildOptions, CodegenOptions, CommandOutcome, ExportOptions, CSHARP_CODEGEN_ID,
};
use coflow_api::DiagnosticSet;
use coflow_project::{normalize_path, path_to_slash, Project};
use coflow_runtime::{ProjectRuntime, SchemaTextOverride};
use data_get_target::parse_data_get_target;
use serde_json::Value;
use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;

mod cli;
mod cli_output;
mod data_commands;
mod data_get_target;
mod diagnostics;
mod schema_commands;
mod skill_commands;
mod write_file;

use diagnostics::cli_error;

use cli::{
    BuildArgs, CftArgs, CftCheckArgs, CftCommand, CleanArgs, Cli, CodegenArgs, Command, DataArgs,
    DataCommand, ExportArgs, InitArgs, LspArgs, ProjectCheckArgs, SchemaArgs, SchemaCommand,
    SkillArgs, SkillCommand, SkillScopeArgs,
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
        Command::Clean(args) => project_clean(&args),
        Command::Export(args) => export_data(&args),
        Command::Codegen(args) => generate_code(&args),
        Command::Schema(command) => run_schema(&command),
        Command::Data(command) => run_data(&command),
        Command::Skill(command) => run_skill(&command),
    }
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
                output: if args.json {
                    schema_commands::SchemaWriteOutput::Json
                } else {
                    schema_commands::SchemaWriteOutput::Human
                },
            },
        ),
    }
}

fn run_data(command: &DataArgs) -> Result<bool, DiagnosticSet> {
    match &command.command {
        DataCommand::Sources(args) => {
            data_commands::sources(args.config_or_dir.as_deref(), !args.json)
        }
        DataCommand::List(args) => data_commands::list(
            args.config_or_dir.as_deref(),
            args.actual_type.clone(),
            args.file.clone(),
            args.limit,
            args.offset,
            !args.json,
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
                human: !args.json,
            })
        }
        DataCommand::Patch(args) => data_commands::patch(
            args.config_or_dir.as_deref(),
            data_commands::DataPatchInput {
                json: args.patch.clone(),
                file: args.patch_file.clone(),
                stdin: args.stdin,
            },
            !args.json,
        ),
        DataCommand::CreateFile(args) => data_commands::create_file(
            args.config_or_dir.as_deref(),
            args.file.clone(),
            args.actual_type.clone(),
            args.provider.clone(),
            args.sheet.clone(),
            !args.json,
        ),
        DataCommand::CreateTable(args) => data_commands::create_table(
            args.config_or_dir.as_deref(),
            args.source.clone(),
            args.actual_type.clone(),
            args.provider.as_deref(),
            args.sheet.clone(),
            !args.json,
        ),
        DataCommand::SyncHeader(args) => data_commands::sync_header(
            args.config_or_dir.as_deref(),
            args.file.clone(),
            args.actual_type.clone(),
            args.provider.clone(),
            args.sheet.clone(),
            !args.json,
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
                output: if args.json {
                    data_commands::DataWriteOutput::Json
                } else {
                    data_commands::DataWriteOutput::Human
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
    let overrides = if let Some(path) = args.stdin_path.as_deref() {
        let mut source = String::new();
        std::io::stdin()
            .read_to_string(&mut source)
            .map_err(|err| cli_error("CLI-STDIN", format!("failed to read stdin: {err}")))?;
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else {
            project.root_dir.join(path)
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
            project_path(&project, &project.config_path)
        );
    } else {
        write_project_diagnostics(diagnostics, args.json, &project.root_dir)
            .map_err(output_error)?;
    }
    Ok(success)
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
    match check_project(&project, &registry)? {
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
    override_code_namespace(&mut project, CSHARP_CODEGEN_ID, args.namespace.as_deref());
    let root_dir = project.root_dir.clone();
    let config_path = project.config_path.clone();
    let registry = default_provider_registry()?;
    match build_project(
        &project,
        &registry,
        BuildOptions {
            data_out_dir: args.data_out_dir.as_deref(),
            code_out_dir: args.code_out_dir.as_deref(),
        },
    )? {
        CommandOutcome::Success(report) => {
            for target in report.targets {
                println!(
                    "{} data exported to {}",
                    target.data.display_name,
                    display_path(&target.data.dir.display().to_string(), Some(&root_dir))
                );
                if let Some(code) = target.code {
                    println!(
                        "{} code generated to {}",
                        code.display_name,
                        display_path(&code.dir.display().to_string(), Some(&root_dir))
                    );
                }
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

fn project_clean(args: &CleanArgs) -> Result<bool, DiagnosticSet> {
    let project = Project::open_schema_only(args.config_or_dir.as_deref())?;
    let report = clean_project(&project)?;
    println!(
        "Cleaned {} historical generations and {} staging entries from {}",
        report.generations_removed,
        report.staging_removed,
        project_path(&project, &project.root_dir.join(".coflow"))
    );
    Ok(true)
}

fn export_data(args: &ExportArgs) -> Result<bool, DiagnosticSet> {
    let project = Project::open_schema_only(args.config_or_dir.as_deref())?;
    let root_dir = project.root_dir.clone();
    let registry = default_provider_registry()?;
    match export_project_data(
        &project,
        &registry,
        &args.output_type,
        ExportOptions {
            out_dir: args.out_dir.as_deref(),
        },
    )? {
        CommandOutcome::Success(report) => {
            println!(
                "{} data exported to {}",
                report.display_name,
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

fn generate_code(args: &CodegenArgs) -> Result<bool, DiagnosticSet> {
    let mut project = Project::open_schema_only(args.config_or_dir.as_deref())?;
    override_code_namespace(&mut project, &args.output_type, args.namespace.as_deref());
    let root_dir = project.root_dir.clone();
    let registry = default_provider_registry()?;
    match generate_project_code(
        &project,
        &registry,
        &args.output_type,
        CodegenOptions {
            out_dir: args.out_dir.as_deref(),
        },
    )? {
        CommandOutcome::Success(report) => {
            println!(
                "{} code generated to {}",
                report.display_name,
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

fn override_code_namespace(project: &mut Project, codegen_id: &str, namespace: Option<&str>) {
    let Some(namespace) = namespace else {
        return;
    };
    if let Some(output) = project
        .config
        .outputs
        .targets_mut()
        .iter_mut()
        .filter_map(|target| target.code.as_mut())
        .find(|output| output.output_type == codegen_id)
    {
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
