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
use coflow_codegen_csharp::{generate_csharp_json, CsharpCodegenOptions};
use coflow_excel_loader::{
    load_excel, ExcelDiagnostic, ExcelDiagnostics, ExcelLoadError, ExcelLocation, ExcelSheet,
    ExcelSource,
};
use coflow_json_export::export_json_model;
use coflow_project::{
    compile_schema_project, dedupe_cft_diagnostics, DiagnosticJson, Project, RelatedJson,
};
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
        Command::Cft(command) => match command.command {
            CftCommand::Check(args) => cft_check(args),
            CftCommand::Lsp(args) => cft_lsp(args),
        },
        Command::Check(args) => project_check(args),
        Command::Export(command) => match command.command {
            ExportCommand::Json(args) => export_json(args),
        },
        Command::Codegen(command) => match command.command {
            CodegenCommand::Csharp(args) => codegen_csharp(args),
        },
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
struct ExportArgs {
    #[command(subcommand)]
    command: ExportCommand,
}

#[derive(Debug, Subcommand)]
enum ExportCommand {
    /// Export data as JSON. The project config must declare outputs.data.type: json.
    Json(ExportJsonArgs),
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
    let config_path = dir.join("coflow.yaml");
    if config_path.exists() {
        return Err(format!("`{}` already exists", config_path.display()));
    }
    let config = r#"schema: schema/

sources: []

outputs:
  data:
    type: json
    dir: generated/data
  code:
    type: csharp
    dir: generated/csharp
    namespace: Game.Config
"#;
    fs::write(&config_path, config)
        .map_err(|err| format!("failed to write `{}`: {err}", config_path.display()))?;
    println!("created {}", config_path.display());
    Ok(true)
}

fn cft_check(args: CftCheckArgs) -> Result<bool, String> {
    let project = Project::open(args.config_or_dir.as_deref())?;
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

fn cft_lsp(args: CftLspArgs) -> Result<bool, String> {
    let project = Project::open(args.config_or_dir.as_deref())?;
    coflow_cft_lsp::run(project)
}

fn project_check(args: ProjectCheckArgs) -> Result<bool, String> {
    let project = Project::open(args.config_or_dir.as_deref())?;
    let build = compile_schema_project(&project, None)?;
    let cft_diagnostics = dedupe_cft_diagnostics(build.diagnostics);
    if !cft_diagnostics.is_empty() {
        if args.json {
            write_json_diagnostics(
                cft_diagnostics
                    .iter()
                    .map(|diagnostic| {
                        DiagnosticJson::from_cft(diagnostic, &build.sources, &build.paths)
                    })
                    .collect(),
            )?;
        } else {
            write_human_cft_diagnostics(&cft_diagnostics, &build.sources, &build.paths)?;
        }
        return Ok(false);
    }

    let Some(schema) = build.container else {
        return Err("schema compilation did not produce a container".to_string());
    };
    let sources = excel_sources(&project);
    match load_excel(&schema, &sources) {
        Ok(output) => {
            if let Some(checks) = output.check_diagnostics {
                if args.json {
                    write_json_diagnostics(diagnostics_from_excel_checks(&checks))?;
                } else {
                    write_human_excel_diagnostics(&checks)?;
                }
                Ok(false)
            } else {
                if args.json {
                    write_json_diagnostics(Vec::new())?;
                } else {
                    println!("Project check passed: {}", project.config_path.display());
                }
                Ok(true)
            }
        }
        Err(err) => {
            if args.json {
                write_json_diagnostics(diagnostics_from_excel_error(&err))?;
            } else {
                write_human_excel_error(&err)?;
            }
            Ok(false)
        }
    }
}

fn export_json(args: ExportJsonArgs) -> Result<bool, String> {
    let project = Project::open(args.config_or_dir.as_deref())?;
    let output = project.config.outputs.data.as_ref().ok_or_else(|| {
        "coflow.yaml missing outputs.data; required `type: json` and `dir` for `coflow export json`"
            .to_string()
    })?;
    if output.output_type != "json" {
        return Err(format!(
            "coflow.yaml outputs.data.type is `{}`; required `json` for `coflow export json`",
            output.output_type
        ));
    }
    let dir = args.out_dir.as_deref().map_or_else(
        || project.resolve_path(&output.dir),
        |path| project.resolve_path(path),
    );
    let build = compile_schema_project(&project, None)?;
    let cft_diagnostics = dedupe_cft_diagnostics(build.diagnostics);
    if !cft_diagnostics.is_empty() {
        write_human_cft_diagnostics(&cft_diagnostics, &build.sources, &build.paths)?;
        return Ok(false);
    }
    let Some(schema) = build.container else {
        return Err("schema compilation did not produce a container".to_string());
    };
    let sources = excel_sources(&project);
    let load_output = match load_excel(&schema, &sources) {
        Ok(output) => output,
        Err(err) => {
            write_human_excel_error(&err)?;
            return Ok(false);
        }
    };
    if let Some(checks) = load_output.check_diagnostics {
        write_human_excel_diagnostics(&checks)?;
        return Ok(false);
    }

    let tables = export_json_model(&schema, &load_output.model)
        .map_err(|err| format!("failed to export JSON model: {err}"))?;
    fs::create_dir_all(&dir)
        .map_err(|err| format!("failed to create output dir `{}`: {err}", dir.display()))?;
    for (table, value) in tables {
        let path = dir.join(format!("{table}.json"));
        let file = fs::File::create(&path)
            .map_err(|err| format!("failed to create `{}`: {err}", path.display()))?;
        serde_json::to_writer_pretty(file, &value)
            .map_err(|err| format!("failed to write `{}`: {err}", path.display()))?;
    }
    println!("JSON data exported to {}", dir.display());
    Ok(true)
}

fn codegen_csharp(args: CodegenCsharpArgs) -> Result<bool, String> {
    let project = Project::open(args.config_or_dir.as_deref())?;
    let output = project
        .config
        .outputs
        .code
        .as_ref()
        .ok_or_else(|| {
            "coflow.yaml missing outputs.code; required `type: csharp` and `dir` for `coflow codegen csharp`"
                .to_string()
        })?;
    if output.output_type != "csharp" {
        return Err(format!(
            "coflow.yaml outputs.code.type is `{}`; required `csharp` for `coflow codegen csharp`",
            output.output_type
        ));
    }
    let dir = args.out_dir.as_deref().map_or_else(
        || project.resolve_path(&output.dir),
        |path| project.resolve_path(path),
    );
    let namespace = args
        .namespace
        .as_deref()
        .or(output.namespace.as_deref())
        .unwrap_or("Game.Config");
    let build = compile_schema_project(&project, None)?;
    let cft_diagnostics = dedupe_cft_diagnostics(build.diagnostics);
    if !cft_diagnostics.is_empty() {
        write_human_cft_diagnostics(&cft_diagnostics, &build.sources, &build.paths)?;
        return Ok(false);
    }
    let Some(schema) = build.container else {
        return Err("schema compilation did not produce a container".to_string());
    };

    let options = CsharpCodegenOptions::new(namespace);
    let files = generate_csharp_json(&schema, &options)
        .map_err(|err| format!("failed to generate C# code: {err}"))?;
    fs::create_dir_all(&dir)
        .map_err(|err| format!("failed to create output dir `{}`: {err}", dir.display()))?;
    for file in files {
        let path = dir.join(&file.relative_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("failed to create `{}`: {err}", parent.display()))?;
        }
        fs::write(&path, file.contents)
            .map_err(|err| format!("failed to write `{}`: {err}", path.display()))?;
    }
    println!("C# code generated to {}", dir.display());
    Ok(true)
}

fn excel_sources(project: &Project) -> Vec<ExcelSource> {
    project
        .config
        .sources
        .iter()
        .map(|source| {
            let sheets = source
                .sheets
                .iter()
                .map(|sheet| {
                    let mut out = ExcelSheet::new(sheet.sheet.clone());
                    if let Some(type_name) = &sheet.type_name {
                        out = out.with_type(type_name.clone());
                    }
                    if !sheet.columns.is_empty() {
                        out = out.with_columns(sheet.columns.clone());
                    }
                    out
                })
                .collect();
            ExcelSource::new(project.resolve_path(&source.file), sheets)
        })
        .collect()
}

#[derive(Debug, Default, Serialize)]
struct DiagnosticsOutput {
    diagnostics: Vec<DiagnosticJson>,
}

fn excel_diagnostic_json(diagnostic: &ExcelDiagnostic) -> DiagnosticJson {
    let fallback = ExcelLocation::new("");
    let location = diagnostic
        .primary
        .as_ref()
        .map(|label| &label.location)
        .unwrap_or(&fallback);
    let (line, character) = excel_position(location);
    DiagnosticJson {
        code: diagnostic.source.code.as_str().to_string(),
        stage: diagnostic.source.stage.to_string(),
        severity: "error".to_string(),
        message: diagnostic.source.message.clone(),
        path: location.file.display().to_string(),
        start_line: line,
        start_character: character,
        end_line: line,
        end_character: character.saturating_add(1),
        related: diagnostic
            .related
            .iter()
            .map(|label| excel_related_json(&label.location, label.message.clone()))
            .collect(),
    }
}

fn excel_error_json(
    code: impl Into<String>,
    stage: impl Into<String>,
    message: String,
) -> DiagnosticJson {
    DiagnosticJson {
        code: code.into(),
        stage: stage.into(),
        severity: "error".to_string(),
        message,
        path: String::new(),
        start_line: 0,
        start_character: 0,
        end_line: 0,
        end_character: 1,
        related: Vec::new(),
    }
}

fn excel_location_json(
    code: impl Into<String>,
    stage: impl Into<String>,
    message: String,
    location: &ExcelLocation,
) -> DiagnosticJson {
    let (line, character) = excel_position(location);
    DiagnosticJson {
        code: code.into(),
        stage: stage.into(),
        severity: "error".to_string(),
        message,
        path: location.file.display().to_string(),
        start_line: line,
        start_character: character,
        end_line: line,
        end_character: character.saturating_add(1),
        related: Vec::new(),
    }
}

fn excel_related_json(location: &ExcelLocation, label: Option<String>) -> RelatedJson {
    let (line, character) = excel_position(location);
    RelatedJson {
        path: location.file.display().to_string(),
        start_line: line,
        start_character: character,
        end_line: line,
        end_character: character.saturating_add(1),
        label,
    }
}

fn excel_position(location: &ExcelLocation) -> (usize, usize) {
    (
        location.row.unwrap_or(1).saturating_sub(1),
        location.column.unwrap_or(1).saturating_sub(1),
    )
}

fn diagnostics_from_excel_checks(checks: &ExcelDiagnostics) -> Vec<DiagnosticJson> {
    checks
        .diagnostics
        .iter()
        .map(excel_diagnostic_json)
        .collect()
}

fn diagnostics_from_excel_error(err: &ExcelLoadError) -> Vec<DiagnosticJson> {
    match err {
        ExcelLoadError::OpenWorkbook { file, message } => vec![excel_error_json(
            "EXCEL-OPEN",
            "EXCEL",
            format!("failed to open workbook `{}`: {message}", file.display()),
        )],
        ExcelLoadError::ReadSheet { location, message } => vec![excel_location_json(
            "EXCEL-SHEET",
            "EXCEL",
            message.clone(),
            location,
        )],
        ExcelLoadError::MissingSheet { file, sheet } => vec![excel_error_json(
            "EXCEL-SHEET",
            "EXCEL",
            format!("workbook `{}` is missing sheet `{sheet}`", file.display()),
        )],
        ExcelLoadError::EmptySheet { location } => vec![excel_location_json(
            "EXCEL-SHEET",
            "EXCEL",
            "sheet is empty".to_string(),
            location,
        )],
        ExcelLoadError::UnknownType {
            location,
            type_name,
        } => vec![excel_location_json(
            "EXCEL-TYPE",
            "EXCEL",
            format!("unknown CFT type `{type_name}`"),
            location,
        )],
        ExcelLoadError::UnknownColumn {
            location,
            type_name,
            column,
            field,
        } => vec![excel_location_json(
            "EXCEL-COLUMN",
            "EXCEL",
            format!("column `{column}` maps to unknown field `{field}` on type `{type_name}`"),
            location,
        )],
        ExcelLoadError::DuplicateFieldColumn {
            location,
            field,
            first_column,
            duplicate_column,
        } => vec![excel_location_json(
            "EXCEL-COLUMN",
            "EXCEL",
            format!("field `{field}` is mapped by both `{first_column}` and `{duplicate_column}`"),
            location,
        )],
        ExcelLoadError::CellParse {
            location,
            type_name,
            field,
            diagnostics,
        } => diagnostics
            .diagnostics
            .iter()
            .map(|diag| {
                excel_location_json(
                    format!("CELL-{:?}", diag.code),
                    "CELL",
                    format!(
                        "failed to parse `{type_name}.{field}` cell: {}",
                        diag.message
                    ),
                    location,
                )
            })
            .collect(),
        ExcelLoadError::DataModel(diagnostics) => diagnostics_from_excel_checks(diagnostics),
    }
}

fn write_json_diagnostics(diagnostics: Vec<DiagnosticJson>) -> Result<(), String> {
    serde_json::to_writer(io::stdout().lock(), &DiagnosticsOutput { diagnostics })
        .map_err(|err| format!("failed to write diagnostics JSON: {err}"))?;
    println!();
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

fn write_human_excel_diagnostics(diagnostics: &ExcelDiagnostics) -> Result<(), String> {
    let mut stderr = io::stderr().lock();
    for diagnostic in &diagnostics.diagnostics {
        let location = diagnostic.primary.as_ref().map(|label| &label.location);
        writeln!(
            stderr,
            "{} [{}] {} {}",
            diagnostic.source.code.as_str(),
            diagnostic.source.stage,
            location.map_or_else(String::new, format_excel_location),
            diagnostic.source.message
        )
        .map_err(|err| format!("failed to write diagnostics: {err}"))?;
    }
    Ok(())
}

fn write_human_excel_error(err: &ExcelLoadError) -> Result<(), String> {
    let mut stderr = io::stderr().lock();
    for diagnostic in diagnostics_from_excel_error(err) {
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

fn format_excel_location(location: &ExcelLocation) -> String {
    let mut out = location.file.display().to_string();
    if let Some(sheet) = &location.sheet {
        out.push('#');
        out.push_str(sheet);
    }
    if let Some(row) = location.row {
        out.push(':');
        out.push_str(&row.to_string());
        if let Some(column) = location.column {
            out.push(':');
            out.push_str(&column.to_string());
        }
    }
    out
}
