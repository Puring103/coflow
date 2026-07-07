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
use coflow::commands::{
    build_project, check_project, export_project_data, generate_project_code, BuildOptions,
    CodegenOptions, CommandOutcome, ExportOptions, CSHARP_CODEGEN_ID, JSON_EXPORTER_ID,
    MESSAGEPACK_EXPORTER_ID,
};
use coflow::diagnostics::{diagnostic_json_from_set, DiagnosticJson};
use coflow::{data_commands, schema_commands};
use coflow_cft::CftDiagnostic;
use coflow_engine::RecordCoordinate;
use coflow_project::{compile_schema_project, dedupe_cft_diagnostics, Project};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

mod cli;

use cli::{
    BuildArgs, CftArgs, CftCheckArgs, CftCommand, Cli, CodegenArgs, CodegenCommand,
    CodegenCsharpArgs, Command, DataArgs, DataCommand, ExportArgs, ExportCommand,
    ExportJsonArgs, ExportMessagePackArgs, InitArgs, LspArgs, ProjectCheckArgs, SchemaArgs,
    SchemaCommand,
};

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
        Command::Schema(command) => run_schema(&command),
        Command::Data(command) => run_data(&command),
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

fn run_schema(command: &SchemaArgs) -> Result<bool, String> {
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

fn run_data(command: &DataArgs) -> Result<bool, String> {
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
            let target = parse_data_get_target(&args.target)?;
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

fn init_project(args: InitArgs) -> Result<bool, String> {
    let dir = args.dir.unwrap_or_else(|| PathBuf::from("."));
    let outcome = coflow_project::init_project(&dir)?;
    println!("created {}", outcome.config_path.display());
    Ok(true)
}

#[derive(Debug)]
struct DataGetTarget {
    config_or_dir: Option<PathBuf>,
    selector: Option<RecordCoordinate>,
}

fn parse_data_get_target(values: &[String]) -> Result<DataGetTarget, String> {
    match values {
        [] => Ok(DataGetTarget {
            config_or_dir: None,
            selector: None,
        }),
        [only] if looks_like_config_path(only) => Ok(DataGetTarget {
            config_or_dir: Some(PathBuf::from(only)),
            selector: None,
        }),
        [only] if looks_like_record_selector(only) => Ok(DataGetTarget {
            config_or_dir: None,
            selector: Some(parse_record_selector(only)?),
        }),
        [only] => Ok(DataGetTarget {
            config_or_dir: Some(PathBuf::from(only)),
            selector: None,
        }),
        [config_or_dir, selector] => Ok(DataGetTarget {
            config_or_dir: Some(PathBuf::from(config_or_dir)),
            selector: Some(parse_record_selector(selector)?),
        }),
        _ => Err("data get accepts at most CONFIG_OR_DIR and TYPE.KEY".to_string()),
    }
}

fn looks_like_record_selector(value: &str) -> bool {
    value.split_once('.').is_some_and(|(actual_type, key)| {
        !actual_type.is_empty() && !key.is_empty() && !value.contains('/') && !value.contains('\\')
    })
}

fn looks_like_config_path(value: &str) -> bool {
    let path = Path::new(value);
    if path.exists() || value.contains('/') || value.contains('\\') {
        return true;
    }
    path.extension().is_some_and(|extension| {
        extension.eq_ignore_ascii_case("yaml") || extension.eq_ignore_ascii_case("yml")
    })
}

fn parse_record_selector(value: &str) -> Result<RecordCoordinate, String> {
    let Some((actual_type, key)) = value.split_once('.') else {
        return Err(format!(
            "record selector `{value}` must be written as TYPE.KEY"
        ));
    };
    if actual_type.is_empty() || key.is_empty() {
        return Err(format!(
            "record selector `{value}` must be written as TYPE.KEY"
        ));
    }
    Ok(RecordCoordinate::new(actual_type, key))
}

fn split_keys(keys: &[String]) -> Vec<String> {
    keys.iter()
        .flat_map(|key| key.split(','))
        .filter(|key| !key.is_empty())
        .map(ToOwned::to_owned)
        .collect()
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
    let root_dir = project.root_dir.clone();
    let config_path = project.config_path.clone();
    let registry = coflow_builtins::default_provider_registry().map_err(|err| err.to_string())?;
    match check_project(project, &registry)
        .map_err(|message| relativize_message_paths(&message, &root_dir))?
    {
        CommandOutcome::Success(_) => {
            if args.json {
                write_json_diagnostics(Vec::new())?;
            } else {
                println!(
                    "Project check passed: {}",
                    display_path(&config_path.display().to_string(), Some(&root_dir))
                );
            }
            Ok(true)
        }
        CommandOutcome::Diagnostics(diagnostics) => {
            write_project_diagnostics(diagnostics, args.json, &root_dir)?;
            Ok(false)
        }
    }
}

fn project_build(args: &BuildArgs) -> Result<bool, String> {
    let mut project = Project::open_schema_only(args.config_or_dir.as_deref())?;
    override_code_namespace(&mut project, args.namespace.as_deref());
    let root_dir = project.root_dir.clone();
    let config_path = project.config_path.clone();
    let registry = coflow_builtins::default_provider_registry().map_err(|err| err.to_string())?;
    match build_project(
        project,
        &registry,
        BuildOptions {
            data_out_dir: args.data_out_dir.as_deref(),
            code_out_dir: args.code_out_dir.as_deref(),
        },
    )
    .map_err(|message| relativize_message_paths(&message, &root_dir))?
    {
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
            write_project_diagnostics(diagnostics, false, &root_dir)?;
            Ok(false)
        }
    }
}

fn export_json(args: &ExportJsonArgs) -> Result<bool, String> {
    let project = Project::open_schema_only(args.config_or_dir.as_deref())?;
    let root_dir = project.root_dir.clone();
    let registry = coflow_builtins::default_provider_registry().map_err(|err| err.to_string())?;
    match export_project_data(
        project,
        &registry,
        JSON_EXPORTER_ID,
        ExportOptions {
            out_dir: args.out_dir.as_deref(),
        },
    )
    .map_err(|message| relativize_message_paths(&message, &root_dir))?
    {
        CommandOutcome::Success(report) => {
            println!(
                "JSON data exported to {}",
                display_path(&report.dir.display().to_string(), Some(&root_dir))
            );
            Ok(true)
        }
        CommandOutcome::Diagnostics(diagnostics) => {
            write_project_diagnostics(diagnostics, false, &root_dir)?;
            Ok(false)
        }
    }
}

fn export_messagepack(args: &ExportMessagePackArgs) -> Result<bool, String> {
    let project = Project::open_schema_only(args.config_or_dir.as_deref())?;
    let root_dir = project.root_dir.clone();
    let registry = coflow_builtins::default_provider_registry().map_err(|err| err.to_string())?;
    match export_project_data(
        project,
        &registry,
        MESSAGEPACK_EXPORTER_ID,
        ExportOptions {
            out_dir: args.out_dir.as_deref(),
        },
    )
    .map_err(|message| relativize_message_paths(&message, &root_dir))?
    {
        CommandOutcome::Success(report) => {
            println!(
                "MessagePack data exported to {}",
                display_path(&report.dir.display().to_string(), Some(&root_dir))
            );
            Ok(true)
        }
        CommandOutcome::Diagnostics(diagnostics) => {
            write_project_diagnostics(diagnostics, false, &root_dir)?;
            Ok(false)
        }
    }
}

fn codegen_csharp(args: &CodegenCsharpArgs) -> Result<bool, String> {
    let mut project = Project::open_schema_only(args.config_or_dir.as_deref())?;
    override_code_namespace(&mut project, args.namespace.as_deref());
    let root_dir = project.root_dir.clone();
    let registry = coflow_builtins::default_provider_registry().map_err(|err| err.to_string())?;
    match generate_project_code(
        project,
        &registry,
        CSHARP_CODEGEN_ID,
        CodegenOptions {
            out_dir: args.out_dir.as_deref(),
        },
    )
    .map_err(|message| relativize_message_paths(&message, &root_dir))?
    {
        CommandOutcome::Success(report) => {
            println!(
                "C# code generated to {}",
                display_path(&report.dir.display().to_string(), Some(&root_dir))
            );
            Ok(true)
        }
        CommandOutcome::Diagnostics(diagnostics) => {
            write_project_diagnostics(diagnostics, false, &root_dir)?;
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
